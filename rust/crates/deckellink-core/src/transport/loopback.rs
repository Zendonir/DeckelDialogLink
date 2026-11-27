//! In-memory loopback (TX bridged to RX) for tests and the simulator.

use std::collections::VecDeque;
use std::sync::Arc;

use crate::clock::{CancelToken, Clock};
use crate::model::SerialSettings;
use crate::transport::{ErrorKind, ModemStatus, Transport, TransportError};

pub const LOOPBACK_PORT_NAME: &str = "LOOPBACK (simuliert)";

/// Everything written can be read back. `bridged = false` simulates a missing bridge,
/// `corrupt_every` flips a bit in every n-th byte, `drop_every` loses every n-th byte.
#[derive(Debug)]
pub struct LoopbackTransport {
    settings: SerialSettings,
    clock: Arc<dyn Clock>,
    pub bridged: bool,
    pub corrupt_every: usize,
    pub drop_every: usize,
    buffer: VecDeque<u8>,
    open: bool,
    counter: usize,
    rts: bool,
    dtr: bool,
}

impl LoopbackTransport {
    pub fn new(settings: SerialSettings, clock: Arc<dyn Clock>) -> Self {
        let (rts, dtr) = (settings.initial_rts, settings.initial_dtr);
        Self {
            settings,
            clock,
            bridged: true,
            corrupt_every: 0,
            drop_every: 0,
            buffer: VecDeque::new(),
            open: false,
            counter: 0,
            rts,
            dtr,
        }
    }

    fn require_open(&self) -> Result<(), TransportError> {
        if self.open {
            Ok(())
        } else {
            Err(TransportError::new(ErrorKind::ConnectionLost, "Loopback ist nicht geöffnet.", LOOPBACK_PORT_NAME))
        }
    }
}

impl Transport for LoopbackTransport {
    fn name(&self) -> String {
        LOOPBACK_PORT_NAME.into()
    }

    fn is_open(&self) -> bool {
        self.open
    }

    fn is_simulated(&self) -> bool {
        true
    }

    fn open(&mut self) -> Result<(), TransportError> {
        self.open = true;
        Ok(())
    }

    fn close(&mut self) {
        self.open = false;
        self.buffer.clear();
    }

    fn write(&mut self, data: &[u8], cancel: &CancelToken) -> Result<usize, TransportError> {
        self.require_open()?;
        #[allow(clippy::cast_precision_loss)]
        if self.clock.sleep(data.len() as f64 / self.settings.bytes_per_second().max(1.0), Some(cancel)) {
            return Ok(0);
        }
        if !self.bridged {
            return Ok(data.len());
        }
        for &byte in data {
            self.counter += 1;
            if self.drop_every > 0 && self.counter.is_multiple_of(self.drop_every) {
                continue;
            }
            let value = if self.corrupt_every > 0 && self.counter.is_multiple_of(self.corrupt_every) {
                byte ^ 0x01
            } else {
                byte
            };
            self.buffer.push_back(value);
        }
        Ok(data.len())
    }

    fn read(&mut self, max: usize, cancel: &CancelToken) -> Result<Vec<u8>, TransportError> {
        self.require_open()?;
        if self.buffer.is_empty() {
            #[allow(clippy::cast_precision_loss)]
            self.clock.sleep(self.settings.read_timeout_ms as f64 / 1000.0, Some(cancel));
            return Ok(Vec::new());
        }
        let count = max.min(self.buffer.len());
        Ok(self.buffer.drain(..count).collect())
    }

    fn out_waiting(&mut self) -> usize {
        0
    }

    fn modem_status(&mut self) -> ModemStatus {
        if !self.open {
            return ModemStatus::default();
        }
        ModemStatus {
            cts: Some(self.rts),
            dsr: Some(self.dtr),
            dcd: Some(self.dtr),
            ri: Some(false),
            rts: Some(self.rts),
            dtr: Some(self.dtr),
        }
    }

    fn set_control_lines(&mut self, rts: Option<bool>, dtr: Option<bool>) -> Result<(), TransportError> {
        self.require_open()?;
        if let Some(rts) = rts {
            self.rts = rts;
        }
        if let Some(dtr) = dtr {
            self.dtr = dtr;
        }
        Ok(())
    }

    fn reset_input(&mut self) {
        self.buffer.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::FakeClock;

    #[test]
    fn echoes_with_faults() {
        let clock: Arc<dyn Clock> = Arc::new(FakeClock::default());
        let token = CancelToken::new();
        let mut t = LoopbackTransport::new(SerialSettings::default(), Arc::clone(&clock));
        assert!(t.read(4, &token).is_err());
        t.open().unwrap();
        t.write(b"ABCD", &token).unwrap();
        assert_eq!(t.read(10, &token).unwrap(), b"ABCD");
        assert!(t.read(10, &token).unwrap().is_empty());
        t.corrupt_every = 2;
        t.drop_every = 3;
        t.write(b"abcdef", &token).unwrap();
        assert_eq!(t.read(10, &token).unwrap().len(), 4);
        t.bridged = false;
        t.write(b"x", &token).unwrap();
        t.reset_input();
        assert!(t.read(1, &token).unwrap().is_empty());
        t.set_control_lines(Some(false), None).unwrap();
        assert_eq!(t.modem_status().cts, Some(false));
        t.close();
        assert_eq!(t.modem_status(), ModemStatus::default());
    }
}
