pub fn t(key: &str, lang: &str) -> String {
    match (key, lang) {
        ("sessionWorking", "zh") => "工作中".into(),
        ("sessionThinking", "zh") => "思考中".into(),
        ("sessionJuggling", "zh") => "多任务".into(),
        ("sessionIdle", "zh") => "空闲".into(),
        ("sessionSleeping", "zh") => "睡眠".into(),
        ("sessionJustNow", "zh") => "刚刚".into(),
        ("sessions", "zh") => "会话".into(),
        ("noSessions", "zh") => "没有活跃会话".into(),
        ("dnd", "zh") => "勿扰模式".into(),
        ("size", "zh") => "大小".into(),
        ("language", "zh") => "语言".into(),
        ("about", "zh") => "关于".into(),
        ("quit", "zh") => "退出".into(),
        ("mini", "zh") => "极简模式".into(),
        ("autoStart", "zh") => "随 Claude Code 启动".into(),
        ("hide", "zh") => "隐藏到托盘".into(),
        ("show", "zh") => "显示 Clyde".into(),
        ("macOnly", "zh") => "仅 macOS".into(),
        ("opacity", "zh") => "透明度".into(),
        ("permissionWaitTime", "zh") => "权限等待时间".into(),
        ("lockPosition", "zh") => "锁定位置".into(),
        ("clickThrough", "zh") => "点击穿透".into(),
        ("hideOnFullscreen", "zh") => "全屏时自动隐藏".into(),
        ("autoDndMeetings", "zh") => "会议/共享时自动勿扰".into(),
        ("checkForUpdates", "zh") => "检查更新".into(),
        ("upToDate", "zh") => "已是最新版本".into(),
        ("upToDateDesc", "zh") => "当前版本已经是最新的了".into(),
        ("checkFailed", "zh") => "检查更新失败".into(),
        ("restoreInteraction", "zh") => "恢复交互".into(),
        // English (default)
        ("sessionWorking", _) => "Working".into(),
        ("sessionThinking", _) => "Thinking".into(),
        ("sessionJuggling", _) => "Juggling".into(),
        ("sessionIdle", _) => "Idle".into(),
        ("sessionSleeping", _) => "Sleeping".into(),
        ("sessionJustNow", _) => "just now".into(),
        ("sessions", _) => "Sessions".into(),
        ("noSessions", _) => "No active sessions".into(),
        ("dnd", _) => "Do Not Disturb".into(),
        ("size", _) => "Size".into(),
        ("language", _) => "Language".into(),
        ("about", _) => "About".into(),
        ("quit", _) => "Quit".into(),
        ("mini", _) => "Mini Mode".into(),
        ("autoStart", _) => "Start with Claude Code".into(),
        ("hide", _) => "Hide to Tray".into(),
        ("show", _) => "Show Clyde".into(),
        ("macOnly", _) => "macOS only".into(),
        ("opacity", _) => "Opacity".into(),
        ("permissionWaitTime", _) => "Permission Wait Time".into(),
        ("lockPosition", _) => "Lock Position".into(),
        ("clickThrough", _) => "Click Through".into(),
        ("hideOnFullscreen", _) => "Hide on Fullscreen".into(),
        ("autoDndMeetings", _) => "Auto DND During Meetings".into(),
        ("checkForUpdates", _) => "Check for Updates".into(),
        ("upToDate", _) => "Already Up to Date".into(),
        ("upToDateDesc", _) => "You are running the latest version".into(),
        ("checkFailed", _) => "Update check failed".into(),
        ("restoreInteraction", _) => "Restore Interaction".into(),
        _ => key.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_zh_translation() {
        assert_eq!(t("quit", "zh"), "退出");
    }
    #[test]
    fn test_en_fallback() {
        assert_eq!(t("quit", "en"), "Quit");
    }
    #[test]
    fn test_unknown_key() {
        assert_eq!(t("unknown_key_xyz", "en"), "unknown_key_xyz");
    }
}
