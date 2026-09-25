use crate::{
    models::system::{AppStatus, BackendStatus},
    state::AppState,
};

/// Reports the current status of the application core.
pub fn app_status(state: &AppState) -> AppStatus {
    AppStatus {
        status: BackendStatus::Ready,
        app: state.app_name.clone(),
        version: state.version.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_state() -> AppState {
        AppState {
            app_name: "ReMa".into(),
            version: "0.1.0".into(),
        }
    }

    #[test]
    fn reports_ready_with_app_identity() {
        let status = app_status(&test_state());

        assert_eq!(
            status,
            AppStatus {
                status: BackendStatus::Ready,
                app: "ReMa".into(),
                version: "0.1.0".into(),
            }
        );
    }

    #[test]
    fn serializes_to_the_frontend_contract() {
        let json = serde_json::to_value(app_status(&test_state())).unwrap();

        assert_eq!(
            json,
            serde_json::json!({ "status": "ready", "app": "ReMa", "version": "0.1.0" })
        );
    }
}
