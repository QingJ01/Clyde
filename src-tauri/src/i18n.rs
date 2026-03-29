pub fn t(key: &str, lang: &str) -> String {
    match (key, lang) {
        ("sessionWorking",  "zh") => "工作中".into(),
        ("sessionThinking", "zh") => "思考中".into(),
        ("sessionJuggling", "zh") => "多任务".into(),
        ("sessionIdle",     "zh") => "空闲".into(),
        ("sessionSleeping", "zh") => "睡眠".into(),
        ("sessionJustNow",  "zh") => "刚刚".into(),
        ("noSessions",      "zh") => "没有活跃会话".into(),
        ("dnd",             "zh") => "勿扰模式".into(),
        ("size",            "zh") => "大小".into(),
        ("language",        "zh") => "语言".into(),
        ("quit",            "zh") => "退出".into(),
        ("mini",            "zh") => "极简模式".into(),
        ("autoStart",       "zh") => "随 Claude Code 启动".into(),
        // English (default)
        ("sessionWorking",  _)    => "Working".into(),
        ("sessionThinking", _)    => "Thinking".into(),
        ("sessionJuggling", _)    => "Juggling".into(),
        ("sessionIdle",     _)    => "Idle".into(),
        ("sessionSleeping", _)    => "Sleeping".into(),
        ("sessionJustNow",  _)    => "just now".into(),
        ("noSessions",      _)    => "No active sessions".into(),
        ("dnd",             _)    => "Do Not Disturb".into(),
        ("size",            _)    => "Size".into(),
        ("language",        _)    => "Language".into(),
        ("quit",            _)    => "Quit".into(),
        ("mini",            _)    => "Mini Mode".into(),
        ("autoStart",       _)    => "Start with Claude Code".into(),
        _                         => key.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn test_zh_translation() { assert_eq!(t("quit", "zh"), "退出"); }
    #[test] fn test_en_fallback()    { assert_eq!(t("quit", "en"), "Quit"); }
    #[test] fn test_unknown_key()    { assert_eq!(t("unknown_key_xyz", "en"), "unknown_key_xyz"); }
}
