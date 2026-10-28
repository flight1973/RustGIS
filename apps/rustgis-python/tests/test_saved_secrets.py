"""Binding-specific regressions; build with `maturin develop --features test-stores`."""

import json
import logging
import threading
import warnings

import pytest

import rustgis
from rustgis import _rustgis

pytestmark = pytest.mark.skipif(
    not hasattr(_rustgis, "TestStore"),
    reason="requires --features test-stores",
)


@pytest.fixture
def desktop(tmp_path, monkeypatch):
    monkeypatch.setenv("RUSTGIS_CONFIG_DIR", str(tmp_path))
    connections = {"connections": [{
        "id": "c1", "kind": "s3", "name": "Work", "address": "https://s3.example.org",
        "options": {"credentials": "keys"},
    }]}
    (tmp_path / "prefs.json").write_text(json.dumps({"connections": json.dumps(connections)}))
    yield
    _rustgis.install_test_store(None)


def test_diagnostics_preserve_results_and_errors_and_report_each_session_once(desktop, caplog):
    _rustgis.install_test_store(_rustgis.TestStore(unavailable=True))
    with warnings.catch_warnings(), caplog.at_level(logging.WARNING, logger="rustgis"):
        warnings.simplefilter("error", RuntimeWarning)
        for _ in range(2):
            caplog.clear()
            session = rustgis.Session(use_saved_secrets=True)
            result = session.run(
                "connection.login", id="c1", accessKeyId="KEYID",
                secretAccessKey="private-sentinel", remember=True,
            )
            assert result["loggedIn"] is True
            with pytest.raises(rustgis.RustGISError):
                session.run("connection.login", id="c1")
            records = [record for record in caplog.records if record.name == "rustgis"]
            assert len(records) == 1 and records[0].levelno == logging.WARNING
            assert "private-sentinel" not in caplog.text


def test_blocked_native_io_releases_the_gil_and_serializes_other_sessions(desktop):
    store = _rustgis.TestStore(gated=True)
    _rustgis.install_test_store(store)
    first = rustgis.Session(use_saved_secrets=True)
    second = rustgis.Session()
    outcomes = {}
    reset_started, reset_done = threading.Event(), threading.Event()

    def read():
        try:
            outcomes["read"] = first.run("connection.login", id="c1")
        except BaseException as error:
            outcomes["read"] = error

    def reset():
        reset_started.set()
        try:
            outcomes["reset"] = second.run("project.new", name="After read")
            outcomes["reads_at_reset"] = store.reads
        except BaseException as error:
            outcomes["reset"] = error
        finally:
            reset_done.set()

    reader = threading.Thread(target=read, daemon=True)
    resetter = threading.Thread(target=reset, daemon=True)
    reader.start()
    try:
        # Python can run while Rust is blocked, but another native operation must wait.
        assert store.wait_entered()
        assert store.reads == 0
        resetter.start()
        assert reset_started.wait(30)
        assert not reset_done.wait(0.2), outcomes
    finally:
        store.release()
        reader.join(60)
        if resetter.ident is not None:
            resetter.join(60)
        assert not reader.is_alive() and not resetter.is_alive(), "a call deadlocked"

    assert isinstance(outcomes["read"], rustgis.RustGISError), outcomes
    assert not isinstance(outcomes["reset"], BaseException), outcomes
    assert outcomes["reads_at_reset"] == 1
    assert second.run("info.project")["name"] == "After read"
