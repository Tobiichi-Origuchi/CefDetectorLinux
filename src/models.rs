use serde::Serialize;

#[derive(Clone, Serialize)]
pub struct AppInfo {
    pub file: String,
    pub app_type: String,
    pub size: u64,
    pub is_running: bool,
    pub is_dir: bool,
    pub icon: String,
}

#[derive(Serialize)]
pub struct CliAppInfo<'a> {
    pub file: &'a str,
    pub app_type: &'a str,
    pub size: u64,
    pub is_running: bool,
    pub is_dir: bool,
}
