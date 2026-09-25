//! Scripted [`AccountRuntime`] for service tests.

use std::sync::Mutex;

use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

use super::{AccountRuntime, RuntimeStatus, SignInAttempt, SignInOutcome};
use crate::{error::AppResult, llm::BoxFuture, secrets::Credential};

pub struct FakeAccountRuntime {
    pub status: Mutex<RuntimeStatus>,
    /// Status after a successful sign-in.
    pub signed_in_label: String,
    pub open_url: Option<String>,
    /// Completes the running sign-in (`None` = cancelled by the runtime).
    pub complete: Mutex<Option<oneshot::Sender<SignInOutcome>>>,
    pub sign_outs: Mutex<usize>,
    pub sign_ins: Mutex<usize>,
    pub credential: Option<Credential>,
}

impl FakeAccountRuntime {
    pub fn signed_out() -> Self {
        Self {
            status: Mutex::new(RuntimeStatus::SignedOut),
            signed_in_label: "ana@example.com · Plus".into(),
            open_url: Some("https://auth.example.com/authorize?state=1".into()),
            complete: Mutex::default(),
            sign_outs: Mutex::default(),
            sign_ins: Mutex::default(),
            credential: None,
        }
    }

    pub fn with_status(status: RuntimeStatus) -> Self {
        let runtime = Self::signed_out();
        *runtime.status.lock().unwrap() = status;
        runtime
    }

    /// Finishes the running sign-in as the provider's page would.
    pub fn finish(&self, outcome: SignInOutcome) {
        if outcome == SignInOutcome::Succeeded {
            *self.status.lock().unwrap() = RuntimeStatus::SignedIn {
                label: Some(self.signed_in_label.clone()),
            };
        }
        if let Some(tx) = self.complete.lock().unwrap().take() {
            let _ = tx.send(outcome);
        }
    }
}

impl AccountRuntime for FakeAccountRuntime {
    fn status(&self) -> BoxFuture<'_, AppResult<RuntimeStatus>> {
        Box::pin(async move { Ok(self.status.lock().unwrap().clone()) })
    }

    fn start_sign_in(&self, device_code: bool) -> BoxFuture<'_, AppResult<SignInAttempt>> {
        Box::pin(async move {
            *self.sign_ins.lock().unwrap() += 1;
            let (tx, rx) = oneshot::channel();
            *self.complete.lock().unwrap() = Some(tx);
            let cancel = CancellationToken::new();
            let cancelled = cancel.clone();
            Ok(SignInAttempt {
                open_url: self.open_url.clone(),
                user_code: device_code.then(|| "ABCD-1234".to_string()),
                outcome: Box::pin(async move {
                    tokio::select! {
                        _ = cancelled.cancelled() => SignInOutcome::Cancelled,
                        outcome = rx => outcome.unwrap_or(SignInOutcome::Cancelled),
                    }
                }),
                cancel,
            })
        })
    }

    fn sign_out(&self) -> BoxFuture<'_, AppResult<()>> {
        Box::pin(async move {
            *self.sign_outs.lock().unwrap() += 1;
            *self.status.lock().unwrap() = RuntimeStatus::SignedOut;
            Ok(())
        })
    }

    fn credential(&self) -> BoxFuture<'_, AppResult<Option<Credential>>> {
        Box::pin(async move { Ok(self.credential.clone()) })
    }

    fn supports_device_code(&self) -> bool {
        true
    }
}
