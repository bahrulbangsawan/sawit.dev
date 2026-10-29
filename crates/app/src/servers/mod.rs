//! PostgreSQL server manager: instance list, lifecycle controls, live logs and
//! port-conflict warnings.

mod detail;
mod list;
mod store;

pub use detail::ServerDetail;
pub use list::{ServerList, status_color};
pub use store::{ServerStore, ShowServerTab, Status};
