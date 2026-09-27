//! XING: its published developer offering is Login with XING (a website
//! plugin bound to a registered domain), Apply with XING and E-Recruiting
//! integrations; no sign-in for desktop apps and no new API applications.
//! ReMa therefore shows XING as not available: it never asks for a XING
//! password or cookies and never reads XING pages. Should an approved
//! integration exist later, it joins here and in the capability registry.

use crate::models::connectors::Capability;

/// Why XING cannot be connected (shown on its card).
pub const UNAVAILABLE: &str = "XING offers no sign-in for desktop apps and no API access for \
    ReMa. Company, job and public people research work without it.";

/// No XING capability is available to ReMa.
pub fn allows(_capability: Capability, _granted: &[String]) -> bool {
    false
}
