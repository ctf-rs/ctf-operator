mod challenge;
mod index;
mod login;
mod not_open;
mod scoreboard;

pub use challenge::ChallengeDetailPage;
pub use index::IndexPage;
pub use login::LoginPage;
pub use not_open::CtfNotOpenPage;
pub use scoreboard::{ScoreboardHiddenPage, ScoreboardPage};
