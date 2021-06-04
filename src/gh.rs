use std::{
    fmt::{self, Debug, Formatter},
    sync::Arc,
};

use github::pulls::PullRequestHandler;

pub type InnerGh = Arc<github::Octocrab>;

struct GhData<'a> {
    pulls: PullRequestHandler<'a>,
}

#[derive(Clone)]
pub struct Gh<'a> {
    data: Arc<GhData<'a>>,
}

impl<'a> Debug for Gh<'a> {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "github client")
    }
}

impl<'a> Gh<'a> {
    /// Creates a new GitHub client for usage with nixpkgs.
    ///
    /// Note: Since this function uses `Box::leak`, you should only call it once.
    pub fn new(gh: InnerGh) -> Self {
        let gh = Box::leak(Box::new(gh));
        let data = GhData { pulls: pulls(gh) };

        Self {
            data: Arc::new(data),
        }
    }

    #[inline(always)]
    pub fn pulls(&self) -> &PullRequestHandler<'a> {
        &self.data.pulls
    }
}

pub fn pulls(gh: &InnerGh) -> PullRequestHandler {
    gh.pulls("NixOS", "nixpkgs")
}
