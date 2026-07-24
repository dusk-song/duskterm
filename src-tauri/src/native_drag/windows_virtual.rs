use std::{
    ffi::c_void,
    mem::ManuallyDrop,
    ptr,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex, OnceLock,
    },
    time::Duration,
};

use windows::{
    core::{implement, Error, HRESULT, HSTRING},
    Win32::{
        Foundation::{
            BOOL, DRAGDROP_S_CANCEL, DRAGDROP_S_DROP, DRAGDROP_S_USEDEFAULTCURSORS, DV_E_FORMATETC,
            E_NOTIMPL, OLE_E_ADVISENOTSUPPORTED, POINT, STG_E_ACCESSDENIED, STG_E_INVALIDFUNCTION,
            STG_E_READFAULT, S_FALSE, S_OK,
        },
        Storage::FileSystem::FILE_ATTRIBUTE_NORMAL,
        System::{
            Com::{
                IAdviseSink, IAgileObject, IAgileObject_Impl, IBindCtx, IDataObject,
                IDataObject_Impl, IEnumFORMATETC, IEnumSTATDATA, ISequentialStream_Impl, IStream,
                IStream_Impl, DATADIR_GET, DVASPECT_CONTENT, FORMATETC, LOCKTYPE, STATFLAG,
                STATSTG, STGC, STGM, STGMEDIUM, STGMEDIUM_0, STGM_READ, STGTY_STREAM, STREAM_SEEK,
                STREAM_SEEK_CUR, STREAM_SEEK_SET, TYMED_HGLOBAL, TYMED_ISTREAM,
            },
            DataExchange::RegisterClipboardFormatW,
            Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE, GMEM_ZEROINIT},
            Ole::{
                DoDragDrop, IDropSource, IDropSource_Impl, OleInitialize, DROPEFFECT,
                DROPEFFECT_COPY,
            },
            SystemServices::{MK_LBUTTON, MODIFIERKEYS_FLAGS},
        },
        UI::{
            Shell::{
                IDataObjectAsyncCapability, IDataObjectAsyncCapability_Impl, SHCreateStdEnumFmtEtc,
                CFSTR_FILECONTENTS, CFSTR_FILEDESCRIPTORW, CFSTR_PREFERREDDROPEFFECT,
                FD_ATTRIBUTES, FD_FILESIZE, FD_UNICODE, FILEDESCRIPTORW,
            },
            WindowsAndMessaging::GetCursorPos,
        },
    },
};

use crate::{
    connection_log,
    sftp::{SftpStreamBridge, SftpStreamCompletion, SftpStreamMessage},
};

static OLE_INITIALIZED: OnceLock<Result<(), String>> = OnceLock::new();

pub struct VirtualDragOutcome {
    pub dropped: bool,
    pub cursor_x: i32,
    pub cursor_y: i32,
}

struct StreamState {
    receiver: crossbeam_channel::Receiver<SftpStreamMessage>,
    cancel: Arc<std::sync::atomic::AtomicBool>,
    completion: Arc<StreamCompletionTracker>,
    pending: Vec<u8>,
    pending_offset: usize,
    position: u64,
    ended: bool,
    fully_consumed: bool,
}

#[derive(Default)]
struct CompletionState {
    data_consumed: bool,
    drop_finished: bool,
    drop_accepted: bool,
    async_mode: bool,
    operation_started: bool,
    operation_finished: bool,
    operation_succeeded: bool,
    notified: bool,
}

struct StreamCompletionTracker {
    sender: crossbeam_channel::Sender<SftpStreamCompletion>,
    state: Mutex<CompletionState>,
}

impl StreamCompletionTracker {
    fn new(sender: crossbeam_channel::Sender<SftpStreamCompletion>) -> Self {
        Self {
            sender,
            state: Mutex::new(CompletionState::default()),
        }
    }

    fn notify_if_ready(&self, state: &mut CompletionState) {
        if state.notified
            || !state.data_consumed
            || !state.drop_finished
            || !state.drop_accepted
            || (state.operation_started
                && (!state.operation_finished || !state.operation_succeeded))
        {
            return;
        }
        state.notified = true;
        let _ = self.sender.try_send(SftpStreamCompletion::Consumed);
    }

    fn data_consumed(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.data_consumed = true;
            self.notify_if_ready(&mut state);
        }
    }

    fn drop_finished(&self, accepted: bool, error: Option<String>) {
        if let Ok(mut state) = self.state.lock() {
            state.drop_finished = true;
            state.drop_accepted = accepted;
            if !accepted && !state.notified {
                state.notified = true;
                let message = match error {
                    Some(error) => SftpStreamCompletion::Failed(error),
                    None => SftpStreamCompletion::Cancelled,
                };
                let _ = self.sender.try_send(message);
                return;
            }
            self.notify_if_ready(&mut state);
        }
    }

    fn failed(&self, error: String) {
        if let Ok(mut state) = self.state.lock() {
            if state.notified {
                return;
            }
            state.notified = true;
            let _ = self.sender.try_send(SftpStreamCompletion::Failed(error));
        }
    }

    fn cancelled(&self) {
        if let Ok(mut state) = self.state.lock() {
            if state.notified {
                return;
            }
            state.notified = true;
            let _ = self.sender.try_send(SftpStreamCompletion::Cancelled);
        }
    }

    fn set_async_mode(&self, enabled: bool) {
        if let Ok(mut state) = self.state.lock() {
            state.async_mode = enabled;
        }
    }

    fn async_mode(&self) -> bool {
        self.state
            .lock()
            .map(|state| state.async_mode)
            .unwrap_or(false)
    }

    fn start_operation(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.operation_started = true;
            state.operation_finished = false;
            state.operation_succeeded = false;
        }
    }

    fn in_operation(&self) -> bool {
        self.state
            .lock()
            .map(|state| state.operation_started && !state.operation_finished)
            .unwrap_or(false)
    }

    fn end_operation(&self, succeeded: bool, error: Option<String>) {
        if let Ok(mut state) = self.state.lock() {
            state.operation_finished = true;
            state.operation_succeeded = succeeded;
            if !succeeded && !state.notified {
                state.notified = true;
                let _ = self.sender.try_send(SftpStreamCompletion::Failed(
                    error.unwrap_or_else(|| "Windows 文件拖放操作失败".to_string()),
                ));
                return;
            }
            self.notify_if_ready(&mut state);
        }
    }
}

#[implement(IStream, IAgileObject)]
struct RemoteFileStream {
    state: Arc<Mutex<StreamState>>,
    total_size: u64,
}

impl RemoteFileStream {
    fn new(bridge: SftpStreamBridge, completion: Arc<StreamCompletionTracker>) -> Self {
        Self {
            total_size: bridge.total_size,
            state: Arc::new(Mutex::new(StreamState {
                receiver: bridge.receiver,
                cancel: bridge.cancel,
                completion,
                pending: Vec::new(),
                pending_offset: 0,
                position: 0,
                ended: false,
                fully_consumed: false,
            })),
        }
    }
}

impl IAgileObject_Impl for RemoteFileStream {}

impl Drop for RemoteFileStream {
    fn drop(&mut self) {
        if let Ok(state) = self.state.lock() {
            if !state.fully_consumed {
                state
                    .cancel
                    .store(true, std::sync::atomic::Ordering::Relaxed);
                state.completion.cancelled();
            }
        }
    }
}

#[allow(non_snake_case)]
impl ISequentialStream_Impl for RemoteFileStream {
    fn Read(&self, pv: *mut c_void, cb: u32, pcbread: *mut u32) -> HRESULT {
        if cb > 0 && pv.is_null() {
            return STG_E_READFAULT;
        }

        let mut state = match self.state.lock() {
            Ok(state) => state,
            Err(_) => return STG_E_READFAULT,
        };
        let output = unsafe { std::slice::from_raw_parts_mut(pv.cast::<u8>(), cb as usize) };
        let mut written = 0usize;

        while written < output.len() && !state.ended {
            if state.pending_offset < state.pending.len() {
                let available = state.pending.len() - state.pending_offset;
                let copy_len = available.min(output.len() - written);
                output[written..written + copy_len].copy_from_slice(
                    &state.pending[state.pending_offset..state.pending_offset + copy_len],
                );
                state.pending_offset += copy_len;
                state.position = state.position.saturating_add(copy_len as u64);
                written += copy_len;
                if self.total_size > 0 && state.position >= self.total_size {
                    state.fully_consumed = true;
                    state.completion.data_consumed();
                }
                if state.pending_offset == state.pending.len() {
                    state.pending.clear();
                    state.pending_offset = 0;
                }
                continue;
            }

            match state.receiver.recv_timeout(Duration::from_millis(100)) {
                Ok(SftpStreamMessage::Data(chunk)) => {
                    state.pending = chunk;
                    state.pending_offset = 0;
                }
                Ok(SftpStreamMessage::End) => {
                    state.ended = true;
                    state.fully_consumed = true;
                    state.completion.data_consumed();
                }
                Ok(SftpStreamMessage::Error(error)) => {
                    state.completion.failed(error);
                    unsafe {
                        if !pcbread.is_null() {
                            *pcbread = written as u32;
                        }
                    }
                    return STG_E_READFAULT;
                }
                Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                    if !state.cancel.load(Ordering::Relaxed) {
                        continue;
                    }
                    state.completion.cancelled();
                    unsafe {
                        if !pcbread.is_null() {
                            *pcbread = written as u32;
                        }
                    }
                    return STG_E_READFAULT;
                }
                Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                    state.completion.failed("SFTP 下载流意外关闭".to_string());
                    unsafe {
                        if !pcbread.is_null() {
                            *pcbread = written as u32;
                        }
                    }
                    return STG_E_READFAULT;
                }
            }
        }

        unsafe {
            if !pcbread.is_null() {
                *pcbread = written as u32;
            }
        }
        if written == output.len() {
            S_OK
        } else {
            S_FALSE
        }
    }

    fn Write(&self, _pv: *const c_void, _cb: u32, pcbwritten: *mut u32) -> HRESULT {
        unsafe {
            if !pcbwritten.is_null() {
                *pcbwritten = 0;
            }
        }
        STG_E_ACCESSDENIED
    }
}

#[allow(non_snake_case)]
impl IStream_Impl for RemoteFileStream {
    fn Seek(
        &self,
        dlibmove: i64,
        dworigin: STREAM_SEEK,
        plibnewposition: *mut u64,
    ) -> windows::core::Result<()> {
        let state = self
            .state
            .lock()
            .map_err(|_| Error::from(STG_E_READFAULT))?;
        let supported = (dworigin == STREAM_SEEK_CUR && dlibmove == 0)
            || (dworigin == STREAM_SEEK_SET && dlibmove as u64 == state.position);
        if !supported {
            return Err(Error::from(STG_E_INVALIDFUNCTION));
        }
        unsafe {
            if !plibnewposition.is_null() {
                *plibnewposition = state.position;
            }
        }
        Ok(())
    }

    fn SetSize(&self, _libnewsize: u64) -> windows::core::Result<()> {
        Err(Error::from(STG_E_ACCESSDENIED))
    }

    fn CopyTo(
        &self,
        _pstm: Option<&IStream>,
        _cb: u64,
        _pcbread: *mut u64,
        _pcbwritten: *mut u64,
    ) -> windows::core::Result<()> {
        Err(Error::from(E_NOTIMPL))
    }

    fn Commit(&self, _grfcommitflags: &STGC) -> windows::core::Result<()> {
        Ok(())
    }

    fn Revert(&self) -> windows::core::Result<()> {
        Err(Error::from(E_NOTIMPL))
    }

    fn LockRegion(
        &self,
        _liboffset: u64,
        _cb: u64,
        _dwlocktype: &LOCKTYPE,
    ) -> windows::core::Result<()> {
        Err(Error::from(E_NOTIMPL))
    }

    fn UnlockRegion(
        &self,
        _liboffset: u64,
        _cb: u64,
        _dwlocktype: u32,
    ) -> windows::core::Result<()> {
        Err(Error::from(E_NOTIMPL))
    }

    fn Stat(&self, pstatstg: *mut STATSTG, _grfstatflag: &STATFLAG) -> windows::core::Result<()> {
        if pstatstg.is_null() {
            return Err(Error::from(STG_E_READFAULT));
        }
        let stat = STATSTG {
            r#type: STGTY_STREAM.0 as u32,
            cbSize: self.total_size,
            grfMode: STGM(STGM_READ.0),
            ..Default::default()
        };
        unsafe {
            ptr::write(pstatstg, stat);
        }
        Ok(())
    }

    fn Clone(&self) -> windows::core::Result<IStream> {
        Err(Error::from(E_NOTIMPL))
    }
}

#[implement(IDataObject, IDataObjectAsyncCapability, IAgileObject)]
struct VirtualFileDataObject {
    session_id: String,
    file_name: String,
    total_size: u64,
    stream: IStream,
    completion: Arc<StreamCompletionTracker>,
    descriptor_format: u16,
    contents_format: u16,
    preferred_effect_format: u16,
    query_get_data_calls: AtomicUsize,
    get_data_calls: AtomicUsize,
    rejected_format_calls: AtomicUsize,
    enum_format_calls: AtomicUsize,
}

impl Drop for VirtualFileDataObject {
    fn drop(&mut self) {
        connection_log::append(
            &self.session_id,
            format!(
                "sftp native drag format summary query={} get={} rejected={} enum={}",
                self.query_get_data_calls.load(Ordering::Relaxed),
                self.get_data_calls.load(Ordering::Relaxed),
                self.rejected_format_calls.load(Ordering::Relaxed),
                self.enum_format_calls.load(Ordering::Relaxed),
            ),
        );
    }
}

impl VirtualFileDataObject {
    fn new(
        session_id: String,
        file_name: String,
        bridge: SftpStreamBridge,
        completion: Arc<StreamCompletionTracker>,
    ) -> windows::core::Result<Self> {
        let descriptor_format = unsafe { RegisterClipboardFormatW(CFSTR_FILEDESCRIPTORW) };
        let contents_format = unsafe { RegisterClipboardFormatW(CFSTR_FILECONTENTS) };
        let preferred_effect_format =
            unsafe { RegisterClipboardFormatW(CFSTR_PREFERREDDROPEFFECT) };
        if descriptor_format == 0 || contents_format == 0 || preferred_effect_format == 0 {
            return Err(Error::new(
                STG_E_READFAULT,
                HSTRING::from("Unable to register virtual file clipboard formats"),
            ));
        }
        connection_log::append(
            &session_id,
            format!(
                "sftp native drag formats descriptor={} contents={} preferred_effect={}",
                descriptor_format, contents_format, preferred_effect_format
            ),
        );
        let total_size = bridge.total_size;
        let stream: IStream = RemoteFileStream::new(bridge, completion.clone()).into();
        if total_size == 0 {
            completion.data_consumed();
        }
        Ok(Self {
            session_id,
            file_name,
            total_size,
            stream,
            completion,
            descriptor_format: descriptor_format as u16,
            contents_format: contents_format as u16,
            preferred_effect_format: preferred_effect_format as u16,
            query_get_data_calls: AtomicUsize::new(0),
            get_data_calls: AtomicUsize::new(0),
            rejected_format_calls: AtomicUsize::new(0),
            enum_format_calls: AtomicUsize::new(0),
        })
    }

    fn descriptor_format_etc(&self) -> FORMATETC {
        FORMATETC {
            cfFormat: self.descriptor_format,
            ptd: ptr::null_mut(),
            dwAspect: DVASPECT_CONTENT.0,
            lindex: -1,
            tymed: TYMED_HGLOBAL.0 as u32,
        }
    }

    fn contents_format_etc(&self) -> FORMATETC {
        FORMATETC {
            cfFormat: self.contents_format,
            ptd: ptr::null_mut(),
            dwAspect: DVASPECT_CONTENT.0,
            lindex: 0,
            tymed: TYMED_ISTREAM.0 as u32,
        }
    }

    fn preferred_effect_format_etc(&self) -> FORMATETC {
        FORMATETC {
            cfFormat: self.preferred_effect_format,
            ptd: ptr::null_mut(),
            dwAspect: DVASPECT_CONTENT.0,
            lindex: -1,
            tymed: TYMED_HGLOBAL.0 as u32,
        }
    }

    fn is_descriptor_format(&self, format: &FORMATETC) -> bool {
        format.cfFormat == self.descriptor_format
            && format.dwAspect == DVASPECT_CONTENT.0
            && format.tymed & TYMED_HGLOBAL.0 as u32 != 0
    }

    fn is_contents_format(&self, format: &FORMATETC) -> bool {
        format.cfFormat == self.contents_format
            && format.dwAspect == DVASPECT_CONTENT.0
            && matches!(format.lindex, -1 | 0)
            && format.tymed & TYMED_ISTREAM.0 as u32 != 0
    }

    fn is_preferred_effect_format(&self, format: &FORMATETC) -> bool {
        format.cfFormat == self.preferred_effect_format
            && format.dwAspect == DVASPECT_CONTENT.0
            && format.tymed & TYMED_HGLOBAL.0 as u32 != 0
    }

    fn record_format_call(&self, get_data: bool, accepted: bool) {
        if get_data {
            self.get_data_calls.fetch_add(1, Ordering::Relaxed);
        } else {
            self.query_get_data_calls.fetch_add(1, Ordering::Relaxed);
        }
        if !accepted {
            self.rejected_format_calls.fetch_add(1, Ordering::Relaxed);
        }
    }

    fn file_descriptor_hglobal(
        &self,
    ) -> windows::core::Result<windows::Win32::Foundation::HGLOBAL> {
        let allocation_size = std::mem::size_of::<u32>()
            .checked_add(std::mem::size_of::<FILEDESCRIPTORW>())
            .ok_or_else(|| Error::from(STG_E_READFAULT))?;
        let global = unsafe {
            GlobalAlloc(GMEM_MOVEABLE | GMEM_ZEROINIT, allocation_size)
                .map_err(|_| Error::from(STG_E_READFAULT))?
        };
        let memory = unsafe { GlobalLock(global) };
        if memory.is_null() {
            return Err(Error::from(STG_E_READFAULT));
        }

        let mut descriptor = FILEDESCRIPTORW {
            dwFlags: (FD_ATTRIBUTES.0 | FD_FILESIZE.0 | FD_UNICODE.0) as u32,
            dwFileAttributes: FILE_ATTRIBUTE_NORMAL.0,
            nFileSizeHigh: (self.total_size >> 32) as u32,
            nFileSizeLow: self.total_size as u32,
            ..Default::default()
        };
        let mut file_name_buffer = [0u16; 260];
        let mut wide_name_len = 0usize;
        for character in self.file_name.chars() {
            let mut encoded = [0u16; 2];
            let units = character.encode_utf16(&mut encoded);
            if wide_name_len + units.len() > file_name_buffer.len() - 1 {
                break;
            }
            file_name_buffer[wide_name_len..wide_name_len + units.len()].copy_from_slice(units);
            wide_name_len += units.len();
        }
        descriptor.cFileName = file_name_buffer;

        unsafe {
            ptr::write_unaligned(memory.cast::<u32>(), 1);
            ptr::write_unaligned(
                memory
                    .cast::<u8>()
                    .add(std::mem::size_of::<u32>())
                    .cast::<FILEDESCRIPTORW>(),
                descriptor,
            );
            // GlobalUnlock returns zero when the lock count reaches zero, which is the
            // successful case for this single lock. The generated windows binding maps
            // that zero BOOL to Err, so its Result must not be propagated here.
            let _ = GlobalUnlock(global);
        }
        Ok(global)
    }

    fn preferred_effect_hglobal(
        &self,
    ) -> windows::core::Result<windows::Win32::Foundation::HGLOBAL> {
        let global = unsafe {
            GlobalAlloc(GMEM_MOVEABLE | GMEM_ZEROINIT, std::mem::size_of::<u32>())
                .map_err(|_| Error::from(STG_E_READFAULT))?
        };
        let memory = unsafe { GlobalLock(global) };
        if memory.is_null() {
            return Err(Error::from(STG_E_READFAULT));
        }
        unsafe {
            ptr::write_unaligned(memory.cast::<u32>(), DROPEFFECT_COPY.0);
            let _ = GlobalUnlock(global);
        }
        Ok(global)
    }
}

#[allow(non_snake_case)]
impl IDataObject_Impl for VirtualFileDataObject {
    fn GetData(&self, pformatetc: *const FORMATETC) -> windows::core::Result<STGMEDIUM> {
        let format = unsafe { pformatetc.as_ref() }.ok_or_else(|| Error::from(DV_E_FORMATETC))?;
        if self.is_descriptor_format(format) {
            self.record_format_call(true, true);
            return Ok(STGMEDIUM {
                tymed: TYMED_HGLOBAL.0 as u32,
                u: STGMEDIUM_0 {
                    hGlobal: self.file_descriptor_hglobal()?,
                },
                pUnkForRelease: ManuallyDrop::new(None),
            });
        }
        if self.is_contents_format(format) {
            self.record_format_call(true, true);
            return Ok(STGMEDIUM {
                tymed: TYMED_ISTREAM.0 as u32,
                u: STGMEDIUM_0 {
                    pstm: ManuallyDrop::new(Some(self.stream.clone())),
                },
                pUnkForRelease: ManuallyDrop::new(None),
            });
        }
        if self.is_preferred_effect_format(format) {
            self.record_format_call(true, true);
            return Ok(STGMEDIUM {
                tymed: TYMED_HGLOBAL.0 as u32,
                u: STGMEDIUM_0 {
                    hGlobal: self.preferred_effect_hglobal()?,
                },
                pUnkForRelease: ManuallyDrop::new(None),
            });
        }
        self.record_format_call(true, false);
        Err(Error::from(DV_E_FORMATETC))
    }

    fn GetDataHere(
        &self,
        _pformatetc: *const FORMATETC,
        _pmedium: *mut STGMEDIUM,
    ) -> windows::core::Result<()> {
        Err(Error::from(DV_E_FORMATETC))
    }

    fn QueryGetData(&self, pformatetc: *const FORMATETC) -> HRESULT {
        match unsafe { pformatetc.as_ref() } {
            Some(format) => {
                let accepted = self.is_descriptor_format(format)
                    || self.is_contents_format(format)
                    || self.is_preferred_effect_format(format);
                self.record_format_call(false, accepted);
                if accepted {
                    S_OK
                } else {
                    DV_E_FORMATETC
                }
            }
            _ => DV_E_FORMATETC,
        }
    }

    fn GetCanonicalFormatEtc(
        &self,
        _pformatectin: *const FORMATETC,
        pformatetcout: *mut FORMATETC,
    ) -> HRESULT {
        unsafe {
            if let Some(format) = pformatetcout.as_mut() {
                format.ptd = ptr::null_mut();
            }
        }
        E_NOTIMPL
    }

    fn SetData(
        &self,
        _pformatetc: *const FORMATETC,
        _pmedium: *const STGMEDIUM,
        _frelease: BOOL,
    ) -> windows::core::Result<()> {
        Err(Error::from(E_NOTIMPL))
    }

    fn EnumFormatEtc(&self, dwdirection: u32) -> windows::core::Result<IEnumFORMATETC> {
        self.enum_format_calls.fetch_add(1, Ordering::Relaxed);
        if dwdirection != DATADIR_GET.0 as u32 {
            return Err(Error::from(E_NOTIMPL));
        }
        unsafe {
            SHCreateStdEnumFmtEtc(&[
                self.descriptor_format_etc(),
                self.contents_format_etc(),
                self.preferred_effect_format_etc(),
            ])
        }
    }

    fn DAdvise(
        &self,
        _pformatetc: *const FORMATETC,
        _advf: u32,
        _padvsink: Option<&IAdviseSink>,
    ) -> windows::core::Result<u32> {
        Err(Error::from(OLE_E_ADVISENOTSUPPORTED))
    }

    fn DUnadvise(&self, _dwconnection: u32) -> windows::core::Result<()> {
        Err(Error::from(OLE_E_ADVISENOTSUPPORTED))
    }

    fn EnumDAdvise(&self) -> windows::core::Result<IEnumSTATDATA> {
        Err(Error::from(OLE_E_ADVISENOTSUPPORTED))
    }
}

impl IDataObjectAsyncCapability_Impl for VirtualFileDataObject {
    fn SetAsyncMode(&self, fdoopasync: BOOL) -> windows::core::Result<()> {
        self.completion.set_async_mode(fdoopasync.as_bool());
        Ok(())
    }

    fn GetAsyncMode(&self) -> windows::core::Result<BOOL> {
        Ok(BOOL::from(self.completion.async_mode()))
    }

    fn StartOperation(&self, _pbcreserved: Option<&IBindCtx>) -> windows::core::Result<()> {
        self.completion.start_operation();
        Ok(())
    }

    fn InOperation(&self) -> windows::core::Result<BOOL> {
        Ok(BOOL::from(self.completion.in_operation()))
    }

    fn EndOperation(
        &self,
        hresult: HRESULT,
        _pbcreserved: Option<&IBindCtx>,
        dweffects: u32,
    ) -> windows::core::Result<()> {
        let copied = dweffects & DROPEFFECT_COPY.0 != 0;
        let succeeded = hresult.is_ok() && copied;
        self.completion.end_operation(
            succeeded,
            (!succeeded).then(|| {
                format!(
                    "Windows 异步文件拖放失败: hresult={} effect={}",
                    hresult.0, dweffects
                )
            }),
        );
        Ok(())
    }
}

impl IAgileObject_Impl for VirtualFileDataObject {}

#[implement(IDropSource)]
struct VirtualFileDropSource;

#[allow(non_snake_case)]
impl IDropSource_Impl for VirtualFileDropSource {
    fn QueryContinueDrag(&self, fescapepressed: BOOL, grfkeystate: MODIFIERKEYS_FLAGS) -> HRESULT {
        if fescapepressed.as_bool() {
            DRAGDROP_S_CANCEL
        } else if (grfkeystate & MK_LBUTTON) == MODIFIERKEYS_FLAGS(0) {
            DRAGDROP_S_DROP
        } else {
            S_OK
        }
    }

    fn GiveFeedback(&self, _dweffect: DROPEFFECT) -> HRESULT {
        DRAGDROP_S_USEDEFAULTCURSORS
    }
}

pub fn start_virtual_file_drag(
    session_id: String,
    file_name: String,
    bridge: SftpStreamBridge,
) -> Result<VirtualDragOutcome, String> {
    OLE_INITIALIZED
        .get_or_init(|| unsafe { OleInitialize(None).map_err(|error| error.to_string()) })
        .clone()?;

    connection_log::append(
        &session_id,
        format!(
            "sftp native drag start file={} size={}",
            file_name, bridge.total_size
        ),
    );
    let completion = Arc::new(StreamCompletionTracker::new(bridge.completion.clone()));
    let data_object: IDataObject = VirtualFileDataObject::new(
        session_id.clone(),
        sanitize_file_name(file_name),
        bridge,
        completion.clone(),
    )
    .map_err(|error| error.to_string())?
    .into();
    let drop_source: IDropSource = VirtualFileDropSource.into();
    let mut effect = DROPEFFECT::default();
    let result = unsafe { DoDragDrop(&data_object, &drop_source, DROPEFFECT_COPY, &mut effect) };
    let dropped = is_copy_drop(result, effect);
    let completion_error = if dropped || result == DRAGDROP_S_CANCEL {
        None
    } else {
        Some(format!(
            "Windows 拖放目标未接受文件: hresult={} effect={}",
            result.0, effect.0
        ))
    };
    completion.drop_finished(dropped, completion_error);
    connection_log::append(
        &session_id,
        format!(
            "sftp native drag ended hresult={} effect={} dropped={}",
            result.0, effect.0, dropped
        ),
    );
    let mut cursor = POINT::default();
    unsafe {
        let _ = GetCursorPos(&mut cursor);
    }

    Ok(VirtualDragOutcome {
        dropped,
        cursor_x: cursor.x,
        cursor_y: cursor.y,
    })
}

fn is_copy_drop(result: HRESULT, effect: DROPEFFECT) -> bool {
    result == DRAGDROP_S_DROP && effect.0 & DROPEFFECT_COPY.0 != 0
}

fn sanitize_file_name(file_name: String) -> String {
    let sanitized = file_name
        .chars()
        .map(|character| match character {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            _ => character,
        })
        .collect::<String>();
    let trimmed = sanitized.trim().trim_end_matches(['.', ' ']);
    let mut file_name = if trimmed.is_empty() {
        "download".to_string()
    } else {
        trimmed.to_string()
    };
    let stem = file_name
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || stem
            .strip_prefix("COM")
            .or_else(|| stem.strip_prefix("LPT"))
            .is_some_and(|suffix| suffix.len() == 1 && matches!(suffix.as_bytes()[0], b'1'..=b'9'));
    if reserved {
        file_name.insert(0, '_');
    }
    file_name
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copy_drop_requires_drop_result_and_copy_effect() {
        assert!(is_copy_drop(DRAGDROP_S_DROP, DROPEFFECT_COPY));
        assert!(!is_copy_drop(DRAGDROP_S_DROP, DROPEFFECT::default()));
        assert!(!is_copy_drop(DRAGDROP_S_CANCEL, DROPEFFECT_COPY));
    }

    #[test]
    fn file_name_sanitizer_handles_invalid_and_reserved_names() {
        assert_eq!(sanitize_file_name("a<b>.txt".to_string()), "a_b_.txt");
        assert_eq!(sanitize_file_name("CON".to_string()), "_CON");
        assert_eq!(sanitize_file_name("lpt9.log".to_string()), "_lpt9.log");
        assert_eq!(sanitize_file_name("...".to_string()), "download");
    }

    #[test]
    fn completion_waits_for_consumption_and_drop_acceptance() {
        let (sender, receiver) = crossbeam_channel::bounded(1);
        let tracker = StreamCompletionTracker::new(sender);

        tracker.data_consumed();
        assert!(receiver.try_recv().is_err());

        tracker.drop_finished(true, None);
        assert!(matches!(
            receiver.try_recv(),
            Ok(SftpStreamCompletion::Consumed)
        ));
    }

    #[test]
    fn async_completion_waits_for_end_operation() {
        let (sender, receiver) = crossbeam_channel::bounded(1);
        let tracker = StreamCompletionTracker::new(sender);

        tracker.set_async_mode(true);
        tracker.start_operation();
        tracker.data_consumed();
        tracker.drop_finished(true, None);
        assert!(receiver.try_recv().is_err());

        tracker.end_operation(true, None);
        assert!(matches!(
            receiver.try_recv(),
            Ok(SftpStreamCompletion::Consumed)
        ));
    }
}
