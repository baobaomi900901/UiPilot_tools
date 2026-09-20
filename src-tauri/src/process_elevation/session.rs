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
    request_id: Option<String>,
}

pub(crate) struct BoundSession {
    state: Session,
}

impl BoundSession {
    pub(crate) fn new() -> Self {
        Self {
            state: Session::new(),
        }
    }
    pub(crate) fn state(&self) -> Phase {
        self.state.phase()
    }
    #[cfg(windows)]
    pub(crate) fn authenticate(
        &mut self,
        _proof: crate::process_elevation::peer::AuthenticationProof,
    ) -> Result<(), &'static str> {
        self.state.authenticate()
    }

    #[cfg(not(windows))]
    pub(crate) fn authenticate(&mut self) -> Result<(), &'static str> {
        self.state.authenticate()
    }
    pub(crate) fn receive_request<R: std::io::Read>(
        &mut self,
        reader: &mut R,
    ) -> Result<crate::process_elevation::Request, &'static str> {
        let request = crate::process_elevation::read_validated_request(reader)?;
        self.state.accept_request(&request)?;
        Ok(request)
    }
    pub(crate) fn mark_planned(&mut self) -> Result<(), &'static str> {
        self.state.planned()
    }
    pub(crate) fn confirm(&mut self) -> Result<(), &'static str> {
        self.state.confirm()
    }
    pub(crate) fn send_terminal<W: std::io::Write>(
        &mut self,
        writer: &mut W,
        result: crate::process_elevation::TerminalResult,
    ) -> Result<(), &'static str> {
        self.state.terminal()?;
        let request_id = self
            .state
            .request_id()
            .ok_or("ProtocolRejected")?
            .to_owned();
        crate::process_elevation::write_frame(
            writer,
            &crate::process_elevation::Terminal {
                version: 1,
                request_id,
                result,
            },
        )
    }
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
            request_id: None,
        }
    }
    pub(crate) fn phase(&self) -> Phase {
        self.phase
    }
    pub(crate) fn request_id(&self) -> Option<&str> {
        self.request_id.as_deref()
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
    pub(crate) fn accept_request(
        &mut self,
        request: &crate::process_elevation::Request,
    ) -> Result<(), &'static str> {
        crate::process_elevation::validate_request(request)?;
        self.receive_request()?;
        self.request_id = Some(request.request_id.clone());
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

    #[test]
    fn bound_session_reads_validated_request_and_emits_one_terminal_frame() {
        let request = crate::process_elevation::Request {
            version: 1,
            request_id: "d".repeat(64),
            scope: crate::process_elevation::Scope::Single,
            operation: crate::process_elevation::Operation::Terminate,
            root: crate::process_elevation::protocol::RootSelector {
                pid: 9,
                creation_time: 13,
            },
        };
        let mut input = Vec::new();
        crate::process_elevation::write_frame(&mut input, &request).unwrap();
        let mut session = BoundSession::new();
        session
            .authenticate(crate::process_elevation::peer::AuthenticationProof::test_only())
            .unwrap();
        assert_eq!(
            session.receive_request(&mut input.as_slice()).unwrap(),
            request
        );
        session.mark_planned().unwrap();
        session.confirm().unwrap();
        let mut output = Vec::new();
        session
            .send_terminal(
                &mut output,
                crate::process_elevation::TerminalResult::Cancelled,
            )
            .unwrap();
        let terminal: crate::process_elevation::Terminal =
            crate::process_elevation::decode_frame(&output).unwrap();
        assert_eq!(terminal.request_id, request.request_id);
        assert_eq!(
            session.send_terminal(
                &mut output,
                crate::process_elevation::TerminalResult::OutcomeUnknown
            ),
            Err("ProtocolRejected")
        );
    }

    #[test]
    fn terminal_is_consumed_before_transport_failure_and_cannot_retry() {
        struct FailingWriter;
        impl std::io::Write for FailingWriter {
            fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
                Err(std::io::Error::other("closed"))
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Err(std::io::Error::other("closed"))
            }
        }
        let request = crate::process_elevation::Request {
            version: 1,
            request_id: "e".repeat(64),
            scope: crate::process_elevation::Scope::Single,
            operation: crate::process_elevation::Operation::Terminate,
            root: crate::process_elevation::protocol::RootSelector {
                pid: 9,
                creation_time: 13,
            },
        };
        let mut input = Vec::new();
        crate::process_elevation::write_frame(&mut input, &request).unwrap();
        let mut session = BoundSession::new();
        session
            .authenticate(crate::process_elevation::peer::AuthenticationProof::test_only())
            .unwrap();
        session.receive_request(&mut input.as_slice()).unwrap();
        session.mark_planned().unwrap();
        session.confirm().unwrap();
        assert_eq!(
            session.send_terminal(
                &mut FailingWriter,
                crate::process_elevation::TerminalResult::Cancelled
            ),
            Err("TransportUnavailable")
        );
        assert_eq!(session.state(), Phase::Terminal);
        assert_eq!(
            session.send_terminal(
                &mut Vec::new(),
                crate::process_elevation::TerminalResult::OutcomeUnknown
            ),
            Err("ProtocolRejected")
        );
    }

    #[test]
    fn accepted_request_is_bound_to_one_validated_id() {
        let mut session = Session::default();
        session.authenticate().unwrap();
        let request = crate::process_elevation::Request {
            version: 1,
            request_id: "b".repeat(64),
            scope: crate::process_elevation::Scope::Single,
            operation: crate::process_elevation::Operation::Terminate,
            root: crate::process_elevation::protocol::RootSelector {
                pid: 9,
                creation_time: 13,
            },
        };
        session.accept_request(&request).unwrap();
        assert_eq!(session.request_id(), Some(request.request_id.as_str()));
        assert_eq!(session.accept_request(&request), Err("ProtocolRejected"));
    }

    #[test]
    fn invalid_request_does_not_mutate_phase_or_bound_id() {
        let mut session = Session::default();
        session.authenticate().unwrap();
        let mut invalid = crate::process_elevation::Request {
            version: 1,
            request_id: "c".repeat(64),
            scope: crate::process_elevation::Scope::Single,
            operation: crate::process_elevation::Operation::Terminate,
            root: crate::process_elevation::protocol::RootSelector {
                pid: 9,
                creation_time: 13,
            },
        };
        invalid.root.pid = 0;
        assert_eq!(session.accept_request(&invalid), Err("ProtocolRejected"));
        assert_eq!(session.phase(), Phase::AwaitingRequest);
        assert_eq!(session.request_id(), None);
    }
}
