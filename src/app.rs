pub enum View {
    MyIssues,
    Project,
    Detail,
}

pub struct App {
    pub running: bool,
    pub view: View,
    pub api_key: String,
}

impl App {
    pub fn new(api_key: String) -> Self {
        Self {
            running: true,
            view: View::MyIssues,
            api_key,
        }
    }
}
