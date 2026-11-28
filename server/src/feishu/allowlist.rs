pub fn parse_allowed_chat_ids(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

pub fn chat_is_allowed(chat_id: &str, allowed: &[String]) -> bool {
    allowed.iter().any(|item| item == chat_id)
}

pub fn is_allowed_group(chat_id: &str, chat_type: &str, allowed: &[String]) -> bool {
    chat_type == "group" && chat_is_allowed(chat_id, allowed)
}
