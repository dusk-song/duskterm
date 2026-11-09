use serde::Deserialize;

#[derive(Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(super) enum Action {
    Current,
    Change,
    List,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Args {
    pub action: Action,
    pub path: Option<String>,
}

impl Args {
    pub fn destination(&self) -> Result<Option<&str>, String> {
        match (self.action, self.path.as_deref()) {
            (Action::Change, Some(path)) if valid_path(path) => Ok(Some(path)),
            (Action::Current | Action::List, None) => Ok(None),
            _ => Err("仅切换目录需要提供可审阅的 path".into()),
        }
    }
}

pub(super) fn valid_path(path: &str) -> bool {
    !path.trim().is_empty()
        && path.len() <= 4096
        && !path.chars().any(|c| {
            c.is_control()
                || ('\u{202a}'..='\u{202e}').contains(&c)
                || ('\u{2066}'..='\u{2069}').contains(&c)
        })
}

pub(super) const PREVIEW_BYTES: usize = 8192;

#[cfg(windows)]
pub(super) fn cmd(
    action: Action,
    current: Option<String>,
    path: Option<String>,
) -> Result<(String, Option<String>, bool), String> {
    use std::path::{Component, Path, PathBuf};

    let base = || {
        current
            .as_deref()
            .map(Path::new)
            .ok_or("当前 CMD 目录未知，请提供本地绝对路径")
    };
    let directory = match path.as_deref().map(Path::new) {
        Some(path) if path.is_absolute() => path.to_path_buf(),
        Some(path)
            if !path.has_root()
                && !matches!(path.components().next(), Some(Component::Prefix(_))) =>
        {
            base()?.join(path)
        }
        Some(_) => return Err("请使用完整的本地盘符绝对路径，或相对于任务目录的路径".into()),
        None => PathBuf::from(base()?),
    };
    super::cmd_exec::validate_directory(&directory)?;
    let canonical = std::fs::canonicalize(&directory).map_err(|_| "无法核实 CMD 工作目录")?;
    let cwd = canonical.to_str().ok_or("无法显示 CMD 工作目录")?;
    let cwd = cwd.strip_prefix(r"\\?\").unwrap_or(cwd);
    super::cmd_exec::validate_directory(Path::new(cwd))?;
    if action != Action::List {
        return Ok((cwd.into(), None, false));
    }
    let mut names = String::new();
    let entries = std::fs::read_dir(cwd).map_err(|_| "无法列出 CMD 工作目录")?;
    for entry in entries {
        let entry = entry.map_err(|_| "无法读取目录条目")?;
        let name = entry
            .file_name()
            .to_string_lossy()
            .escape_debug()
            .to_string();
        if names.len() + name.len() + 1 > PREVIEW_BYTES {
            return Ok((cwd.into(), Some(names), true));
        }
        names.push_str(&name);
        names.push('\n');
    }
    Ok((cwd.into(), Some(names), false))
}
