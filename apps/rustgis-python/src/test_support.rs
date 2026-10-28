//! Minimal injected store for Python diagnostic and blocking-call regressions. Never uses the OS.

use pyo3::prelude::*;
use rustgis_engine::data::secrets::{MemorySecrets, Secret, SecretError, SecretStore, check_account};
use std::sync::atomic::{AtomicUsize, Ordering::SeqCst};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Default)]
struct Gate {
    entered: bool,
    released: bool,
}

struct State {
    memory: MemorySecrets,
    gated: bool,
    gate: Mutex<Gate>,
    changed: Condvar,
    reads: AtomicUsize,
}

#[derive(Clone)]
#[pyclass(frozen, skip_from_py_object, module = "rustgis._rustgis")]
pub struct TestStore {
    state: Arc<State>,
}

impl SecretStore for TestStore {
    fn load(&self, account: &str) -> Result<Option<Secret>, SecretError> {
        check_account(account)?;
        let released = {
            let mut gate = self.state.gate.lock().unwrap_or_else(|p| p.into_inner());
            gate.entered = true;
            self.state.changed.notify_all();
            if self.state.gated {
                let (gate, _) = self.state.changed.wait_timeout_while(gate, TIMEOUT, |g| !g.released).unwrap_or_else(|p| p.into_inner());
                gate.released
            } else {
                true
            }
        };
        self.state.reads.fetch_add(1, SeqCst);
        if !released {
            return Err(SecretError::Failed("test read gate timed out".into()));
        }
        self.state.memory.load(account)
    }

    fn save(&self, account: &str, value: &Secret) -> Result<(), SecretError> {
        self.state.memory.save(account, value)
    }

    fn delete(&self, account: &str) -> Result<(), SecretError> {
        self.state.memory.delete(account)
    }
}

#[pymethods]
impl TestStore {
    #[new]
    #[pyo3(signature = (unavailable=false, gated=false))]
    fn new(unavailable: bool, gated: bool) -> Self {
        let memory = if unavailable { MemorySecrets::failing(SecretError::Unavailable("test store".into())) } else { MemorySecrets::new() };
        Self {
            state: Arc::new(State {
                memory,
                gated,
                gate: Mutex::new(Gate::default()),
                changed: Condvar::new(),
                reads: AtomicUsize::new(0),
            }),
        }
    }

    #[getter]
    fn reads(&self) -> usize {
        self.state.reads.load(SeqCst)
    }

    fn wait_entered(&self, py: Python<'_>) -> bool {
        let state = self.state.clone();
        py.detach(move || {
            let gate = state.gate.lock().unwrap_or_else(|p| p.into_inner());
            let (gate, _) = state.changed.wait_timeout_while(gate, TIMEOUT, |g| !g.entered).unwrap_or_else(|p| p.into_inner());
            gate.entered
        })
    }

    fn release(&self) {
        self.state.gate.lock().unwrap_or_else(|p| p.into_inner()).released = true;
        self.state.changed.notify_all();
    }
}

static INSTALLED: Mutex<Option<TestStore>> = Mutex::new(None);

pub fn installed() -> TestStore {
    INSTALLED.lock().unwrap_or_else(|p| p.into_inner()).clone().unwrap_or_else(|| TestStore::new(true, false))
}

#[pyfunction]
#[pyo3(signature = (store=None))]
fn install_test_store(store: Option<PyRef<'_, TestStore>>) {
    *INSTALLED.lock().unwrap_or_else(|p| p.into_inner()) = store.map(|store| (*store).clone());
}

pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<TestStore>()?;
    m.add_function(wrap_pyfunction!(install_test_store, m)?)?;
    Ok(())
}
