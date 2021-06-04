use github::{models::pulls::PullRequest, Result as GhResult};

pub type Gh = std::sync::Arc<github::Octocrab>;

pub async fn get_pr(gh: &Gh, number: u64) -> GhResult<PullRequest> {
    gh.pulls("NixOS", "nixpkgs").get(number).await
}
