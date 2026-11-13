use serde::Deserialize;

const MAX_FRAGMENT: usize = 4096;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Args {
    pub path: String,
    old_text: String,
    new_text: String,
}

#[derive(Clone)]
pub(super) struct Preview {
    pub path: String,
    pub content: String,
    pub version: String,
    pub old_text: String,
    pub new_text: String,
    pub line: usize,
}

pub(super) fn preview(path: String, original: &str, args: Args) -> Result<Preview, String> {
    if args.old_text.is_empty() || args.old_text == args.new_text
        || args.old_text.len() > MAX_FRAGMENT || args.new_text.len() > MAX_FRAGMENT
        || [&args.old_text, &args.new_text].iter().any(|text| text.chars().any(|c|
            (c.is_control() && !matches!(c, '\n' | '\t')) || matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
        ))
    {
        return Err("请提供一段明确的旧文本和替换内容，每段最多 4 KiB".into());
    }
    let mut matches = original.match_indices(&args.old_text);
    let (index, _) = matches.next().ok_or("旧文本与当前文件不匹配")?;
    // Also reject overlapping matches.
    let next_char = original[index..].chars().next().unwrap().len_utf8();
    if original[index + next_char..].contains(&args.old_text) {
        return Err("旧文本匹配多处，请补充相邻内容定位到唯一位置".into());
    }
    let mut content = String::with_capacity(original.len() - args.old_text.len() + args.new_text.len());
    content.push_str(&original[..index]);
    content.push_str(&args.new_text);
    content.push_str(&original[index + args.old_text.len()..]);
    if content.len() > super::read_access::MAX_RESULT {
        return Err("修改后文件超过 16 KiB".into());
    }
    Ok(Preview {
        path, content,
        version: crate::sftp::build_text_cas_token(original.as_bytes()),
        old_text: args.old_text, new_text: args.new_text,
        line: original[..index].bytes().filter(|byte| *byte == b'\n').count() + 1,
    })
}
