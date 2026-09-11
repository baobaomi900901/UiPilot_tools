#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Phase {
    Authenticating,
    AwaitingRequest,
    Planning,
    AwaitingConfirmation,
    Executing,
    Terminal,
}

#[derive(Debug, Eq, PartialEq)]
pub struct Session {
    phase: Phase,
    request_received: bool,
}

impl Default for Session {
    fn default() -> Self {
        Self::new()
    }
}

impl Session {
    pub fn new() -> Self {
        Self {
            phase: Phase::Authenticating,
            request_received: false,
        }
    }
    pub fn phase(&self) -> Phase {
        self.phase
    }
    pub fn authenticate(&mut self) -> Result<(), &'static str> {
        self.move_to(Phase::Authenticating, Phase::AwaitingRequest)
    }
    pub fn receive_request(&mut self) -> Result<(), &'static str> {
        if self.phase != Phase::AwaitingRequest || self.request_received {
            return Err("ProtocolRejected");
        }
        self.request_received = true;
        self.phase = Phase::Planning;
        Ok(())
    }
    pub fn planned(&mut self) -> Result<(), &'static str> {
        self.move_to(Phase::Planning, Phase::AwaitingConfirmation)
    }
    pub fn confirm(&mut self) -> Result<(), &'static str> {
        self.move_to(Phase::AwaitingConfirmation, Phase::Executing)
    }
    pub fn terminal(&mut self) -> Result<(), &'static str> {
        self.move_to(Phase::Executing, Phase::Terminal)
    }
    pub fn cancel_before_execution(&mut self) -> Result<(), &'static str> {
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
    fn accepts_only_one_linear_request_and_terminal_result() {
        let mut session = Session::new();
        assert_eq!(session.authenticate(), Ok(()));
        assert_eq!(session.receive_request(), Ok(()));
        assert_eq!(session.receive_request(), Err("ProtocolRejected"));
        assert_eq!(session.planned(), Ok(()));
        assert_eq!(session.confirm(), Ok(()));
        assert_eq!(session.terminal(), Ok(()));
        assert_eq!(session.terminal(), Err("ProtocolRejected"));
        assert_eq!(session.phase(), Phase::Terminal);
    }
    #[test]
    fn cancellation_is_pre_effect_and_cannot_rearm_execution() {
        for phase in [
            Phase::Authenticating,
            Phase::AwaitingRequest,
            Phase::Planning,
            Phase::AwaitingConfirmation,
        ] {
            let mut session = Session::new();
            session.phase = phase;
            assert_eq!(session.cancel_before_execution(), Ok(()));
            assert_eq!(session.receive_request(), Err("ProtocolRejected"));
        }
        let mut executing = Session::new();
        executing.phase = Phase::Executing;
        assert_eq!(executing.cancel_before_execution(), Err("ProtocolRejected"));
    }
}
