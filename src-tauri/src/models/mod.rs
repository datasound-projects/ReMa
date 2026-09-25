pub mod browser;
pub mod chat;
pub mod google;
pub mod jobs;
pub mod profile;
pub mod provider;
pub mod system;
pub mod task;

/// Implements `as_str` / `parse` for enums stored as TEXT columns.
macro_rules! text_enum {
    ($ty:ident { $($variant:ident => $text:literal),+ $(,)? }) => {
        impl $ty {
            pub fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $text),+
                }
            }

            pub fn parse(value: &str) -> Option<Self> {
                match value {
                    $($text => Some(Self::$variant),)+
                    _ => None,
                }
            }
        }
    };
}

pub(crate) use text_enum;
