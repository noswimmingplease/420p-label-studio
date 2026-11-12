//! Bounded, two-phase batch execution. This module does not access a printer.
//!
//! Preflight must capture or refer to immutable inputs that the submit closure
//! will actually use. In particular, the caller must freeze image/template
//! contents, text, formatting and date/time values: a changing path alone is
//! not a snapshot. `accepted` means the submit callback accepted a job, not that
//! a label physically printed. Never automatically retry an uncertain attempt.
use serde_json::Value;
use std::sync::atomic::{AtomicBool, Ordering};

pub const MAX_JOBS: usize = 100;
const MAX_TEXT_CHARS: usize = 500;
const MAX_OBJECTS: usize = 50;

#[derive(Clone, Debug, PartialEq)]
pub struct Job {
    pub text: String,
    pub objects: Vec<Value>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase { Preflight, Submit }

/// Progress counts completed operations, not necessarily physically printed labels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Progress {
    pub phase: Phase,
    pub completed: usize,
    pub total: usize,
    pub accepted: usize,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    Completed { accepted: usize },
    Cancelled { preflighted: usize, accepted: usize },
    InvalidInput(String),
    PreflightFailed { label: usize, reason: String },
    /// `label` is one-based. The failing submit may already have reached the
    /// spooler even if the callback reports an error; do not retry it blindly.
    SubmitFailed { label: usize, accepted: usize, reason: String, last_may_have_been_submitted: bool },
}

/// Validate and render the entire batch before sending its first job. Each
/// successful preflight returns an owned proof/snapshot used for that job's
/// single submit call. Cancellation is cooperative: an in-flight callback is
/// allowed to finish, and no later job is started after cancellation.
pub fn run<T>(
    jobs: &[Job],
    cancel: &AtomicBool,
    mut preflight: impl FnMut(&Job) -> Result<T, String>,
    mut submit: impl FnMut(&Job, &T) -> Result<(), String>,
    mut on_progress: impl FnMut(Progress),
) -> Outcome {
    if jobs.is_empty() || jobs.len() > MAX_JOBS {
        return Outcome::InvalidInput(format!("Choose between 1 and {MAX_JOBS} labels."));
    }
    for (index, job) in jobs.iter().enumerate() {
        if job.text.chars().count() > MAX_TEXT_CHARS {
            return Outcome::InvalidInput(format!("Label {} exceeds the 500-character text limit.", index + 1));
        }
        if job.objects.len() > MAX_OBJECTS || job.objects.iter().any(|object| !object.is_object()) {
            return Outcome::InvalidInput(format!("Label {} contains invalid objects.", index + 1));
        }
    }
    let total = jobs.len();
    let mut proofs = Vec::with_capacity(total);
    on_progress(Progress { phase: Phase::Preflight, completed: 0, total, accepted: 0 });
    for (index, job) in jobs.iter().enumerate() {
        if cancel.load(Ordering::Acquire) { return Outcome::Cancelled { preflighted: proofs.len(), accepted: 0 }; }
        match preflight(job) {
            Ok(proof) => proofs.push(proof),
            Err(reason) => return Outcome::PreflightFailed { label: index + 1, reason },
        }
        on_progress(Progress { phase: Phase::Preflight, completed: proofs.len(), total, accepted: 0 });
    }
    let mut accepted = 0;
    on_progress(Progress { phase: Phase::Submit, completed: 0, total, accepted });
    for (index, (job, proof)) in jobs.iter().zip(proofs.iter()).enumerate() {
        if cancel.load(Ordering::Acquire) { return Outcome::Cancelled { preflighted: total, accepted }; }
        match submit(job, proof) {
            Ok(()) => accepted += 1,
            Err(reason) => return Outcome::SubmitFailed { label: index + 1, accepted, reason, last_may_have_been_submitted: true },
        }
        on_progress(Progress { phase: Phase::Submit, completed: accepted, total, accepted });
    }
    Outcome::Completed { accepted }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::{cell::RefCell, sync::atomic::AtomicUsize};

    fn jobs(count: usize) -> Vec<Job> {
        (0..count).map(|n| Job { text: format!("Label {n}"), objects: vec![json!({"kind":"counter","startAt":n})] }).collect()
    }

    #[test]
    fn preflight_finishes_before_any_submit_and_uses_matching_proofs() {
        let jobs = jobs(3);
        let events = RefCell::new(Vec::new());
        let progress = RefCell::new(Vec::new());
        let result = run(&jobs, &AtomicBool::new(false),
            |job| { events.borrow_mut().push(format!("check {}", job.text)); Ok(job.text.clone()) },
            |job, proof| { assert_eq!(&job.text, proof); events.borrow_mut().push(format!("submit {}", job.text)); Ok(()) },
            |event| progress.borrow_mut().push(event));
        assert_eq!(result, Outcome::Completed { accepted: 3 });
        assert_eq!(&*events.borrow(), &["check Label 0", "check Label 1", "check Label 2", "submit Label 0", "submit Label 1", "submit Label 2"]);
        assert_eq!(progress.borrow().last().unwrap(), &Progress { phase: Phase::Submit, completed: 3, total: 3, accepted: 3 });
    }

    #[test]
    fn failed_preflight_sends_nothing() {
        let calls = AtomicUsize::new(0);
        let result = run(&jobs(3), &AtomicBool::new(false),
            |job| if job.text == "Label 1" { Err("invalid layout".into()) } else { Ok(()) },
            |_, _| { calls.fetch_add(1, Ordering::Relaxed); Ok(()) }, |_| {});
        assert_eq!(result, Outcome::PreflightFailed { label: 2, reason: "invalid layout".into() });
        assert_eq!(calls.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn cancellation_before_and_during_work_does_not_start_later_jobs() {
        let cancel = AtomicBool::new(true);
        let result = run(&jobs(2), &cancel, |_| Ok(()), |_, _| Ok(()), |_| {});
        assert_eq!(result, Outcome::Cancelled { preflighted: 0, accepted: 0 });

        cancel.store(false, Ordering::Release);
        let submits = AtomicUsize::new(0);
        let result = run(&jobs(3), &cancel, |_| Ok(()),
            |_, _| { submits.fetch_add(1, Ordering::Relaxed); cancel.store(true, Ordering::Release); Ok(()) }, |_| {});
        assert_eq!(result, Outcome::Cancelled { preflighted: 3, accepted: 1 });
        assert_eq!(submits.load(Ordering::Relaxed), 1);

        cancel.store(false, Ordering::Release);
        submits.store(0, Ordering::Relaxed);
        let checks = AtomicUsize::new(0);
        let result = run(&jobs(3), &cancel, |_| {
            checks.fetch_add(1, Ordering::Relaxed);
            cancel.store(true, Ordering::Release);
            Ok(())
        }, |_, _| { submits.fetch_add(1, Ordering::Relaxed); Ok(()) }, |_| {});
        assert_eq!(result, Outcome::Cancelled { preflighted: 1, accepted: 0 });
        assert_eq!(checks.load(Ordering::Relaxed), 1);
        assert_eq!(submits.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn failed_submission_is_uncertain_and_never_retried() {
        let submits = AtomicUsize::new(0);
        let result = run(&jobs(4), &AtomicBool::new(false), |_| Ok(()), |_, _| {
            let call = submits.fetch_add(1, Ordering::Relaxed);
            if call == 1 { Err("connection lost".into()) } else { Ok(()) }
        }, |_| {});
        assert_eq!(result, Outcome::SubmitFailed { label: 2, accepted: 1, reason: "connection lost".into(), last_may_have_been_submitted: true });
        assert_eq!(submits.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn input_bounds_are_checked_before_callbacks() {
        let cancel = AtomicBool::new(false);
        let calls = AtomicUsize::new(0);
        let check = |items: &[Job]| run(items, &cancel, |_| { calls.fetch_add(1, Ordering::Relaxed); Ok(()) }, |_, _| Ok(()), |_| {});
        assert!(matches!(check(&[]), Outcome::InvalidInput(_)));
        assert!(matches!(check(&jobs(MAX_JOBS + 1)), Outcome::InvalidInput(_)));
        assert!(matches!(check(&[Job { text: "猫".repeat(501), objects: vec![] }]), Outcome::InvalidInput(_)));
        assert!(matches!(check(&[Job { text: String::new(), objects: vec![json!(null)] }]), Outcome::InvalidInput(_)));
        assert!(matches!(check(&[Job { text: String::new(), objects: vec![json!({}); 51] }]), Outcome::InvalidInput(_)));
        assert_eq!(calls.load(Ordering::Relaxed), 0);
        assert_eq!(run(&jobs(MAX_JOBS), &cancel, |_| Ok(()), |_, _| Ok(()), |_| {}), Outcome::Completed { accepted: MAX_JOBS });
    }

    #[test]
    fn cancel_during_last_success_does_not_misreport_completed_batch() {
        let cancel = AtomicBool::new(false);
        let result = run(&jobs(1), &cancel, |_| Ok(()), |_, _| { cancel.store(true, Ordering::Release); Ok(()) }, |_| {});
        assert_eq!(result, Outcome::Completed { accepted: 1 });
    }
}
