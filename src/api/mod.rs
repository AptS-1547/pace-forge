//! 业务接口层。

pub mod accounts;
pub mod ai;
pub mod audit;
pub mod cheat;
pub mod client;
pub mod coord;
pub mod crosscheck;
pub mod device_table;
pub mod egress;
pub mod fence;
pub mod flow;
pub mod gmttime;
pub mod gt4;
pub mod gt4image;
pub mod guard;
pub mod login;
pub mod obs;
pub mod points;
pub mod policy;
pub mod records;
pub mod semester;
pub mod session;
pub mod start_time;
pub mod submit;

pub use client::{Client, Credentials, DeviceProfile};
pub use login::{LoginOutcome, UserProfile};
