use crate::api::client::LinearApi;

pub enum View {
    MyIssues,
    Project,
    Detail,
}

pub struct App<A: LinearApi> {
    pub running: bool,
    pub view: View,
    pub api: A,
}

impl<A: LinearApi> App<A> {
    pub fn new(api: A) -> Self {
        Self {
            running: true,
            view: View::MyIssues,
            api,
        }
    }
}
