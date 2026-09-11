#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Phase {
    Authenticating,
    AwaitingRequest,
    Planning,
    AwaitingConfirmation,
    Executing,
    Terminal,
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) struct Session {
    phase: Phase,
    request_received: bool,
}

impl Default for Session {
    fn default() -> Self {
        Self::new()
    }
}
impl Session {
    pub(crate) fn new() -> Self {
        Self {
            phase: Phase::Authenticating,
            request_received: false,
        }
    }
    pub(crate) fn phase(&self) -> Phase {
        self.phase
    }
    pub(crate) fn authenticate(&mut self) -> Result<(), &'static str> {
        self.move_to(Phase::Authenticating, Phase::AwaitingRequest)
    }
    pub(crate) fn receive_request(&mut self) -> Result<(), &'static str> {
        if self.phase != Phase::AwaitingRequest || self.request_received {
            return Err("ProtocolRejected");
        }
        self.request_received = true;
        self.phase = Phase::Planning;
        Ok(())
    }
    pub(crate) fn planned(&mut self) -> Result<(), &'static str> {
        self.move_to(Phase::Planning, Phase::AwaitingConfirmation)
    }
    pub(crate) fn confirm(&mut self) -> Result<(), &'static str> {
        self.move_to(Phase::AwaitingConfirmation, Phase::Executing)
    }
    pub(crate) fn terminal(&mut self) -> Result<(), &'static str> {
        self.move_to(Phase::Executing, Phase::Terminal)
    }
    pub(crate) fn cancel_before_execution(&mut self) -> Result<(), &'static str> {
        if matches!(
            self.phase,
            Phase::Authenticating
                | Phase::AwaitingRequest
                | Phase::Planning
                | Phase::AwaitingConfirmation
        ) {
            self.phase = Phase::Terminal;
            Ok(())
        } else {
            Err("ProtocolRejected")
        }
    }
    fn move_to(&mut self, from: Phase, to: Phase) -> Result<(), &'static str> {
        if self.phase != from {
            return Err("ProtocolRejected");
        }
        self.phase = to;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn facade_session_is_linear_and_one_shot() {
        let mut session = Session::default();
        session.authenticate().unwrap();
        session.receive_request().unwrap();
        assert_eq!(session.receive_request(), Err("ProtocolRejected"));
        session.planned().unwrap();
        session.confirm().unwrap();
        session.terminal().unwrap();
        assert_eq!(session.phase(), Phase::Terminal);
        assert_eq!(session.cancel_before_execution(), Err("ProtocolRejected"));
    }
}
