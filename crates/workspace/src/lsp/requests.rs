//! Requests to a language server and their answers. A request is sent
//! through the server's writer thread and returns at once; the answer comes
//! back on the reader thread's channel and is matched here by its id when a
//! frontend polls ([`Workspace::poll_language_servers`]), so the UI thread
//! never waits for a server. A request that is superseded is cancelled with
//! `$/cancelRequest` and forgotten: if its answer comes anyway, nobody is
//! waiting for it and it is dropped. An answer for a document that changed
//! since the request is dropped too.

use super::encoding::PositionEncoding;
use super::server::{Features, ServerId};
use crate::{Document, DocumentId, Event, Workspace};
use serde_json::{Value, json};

/// What a pending request will become.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Hover,
    Completion,
}

/// A request on its way: the server it went to and its id.
pub(crate) type Request = (ServerId, i64);

/// The LSP position of char `offset` of `doc`.
pub(super) fn lsp_position(doc: &Document, offset: usize, encoding: PositionEncoding) -> Value {
    let position = doc.position_of(offset);
    let line = doc.line(position.line);
    json!({
        "line": position.line,
        "character": encoding.units_before(&line, position.column),
    })
}

impl Workspace {
    /// The running server of an open document when `wants` holds for its
    /// features: its id, the document's URI and the position encoding.
    pub(super) fn lsp_target(
        &self,
        id: DocumentId,
        wants: impl Fn(&Features) -> bool,
    ) -> Option<(ServerId, String, PositionEncoding)> {
        let tracked = self.lsp.tracked.get(&id)?;
        let server = self.lsp.server(tracked.server).filter(|s| s.syncs())?;
        wants(&server.features)
            .then(|| (server.id, tracked.uri.clone(), server.capabilities.encoding))
    }

    /// Sends `method` to `server` and remembers it as `kind`.
    pub(super) fn lsp_request(
        &mut self,
        server: ServerId,
        kind: Kind,
        method: &str,
        params: &Value,
    ) -> Option<Request> {
        let id = self.lsp.server_mut(server)?.request(method, params)?;
        self.lsp.pending.insert((server, id), kind);
        Some((server, id))
    }

    /// Cancels a request: the server may stop working on it, and its answer
    /// is dropped when it comes.
    pub(super) fn lsp_cancel(&mut self, request: Request) {
        self.lsp.pending.remove(&request);
        if let Some(server) = self.lsp.server(request.0) {
            server.cancel(request.1);
        }
    }

    /// An answer from `server`; whether it was one this module waits for.
    pub(super) fn lsp_response(&mut self, server: ServerId, message: &Value) -> bool {
        let Some(request) = message["id"].as_i64() else {
            return false;
        };
        let Some(kind) = self.lsp.pending.remove(&(server, request)) else {
            return false;
        };
        let name = self.lsp.server(server).map(super::server::Server::name);
        let span = rustode_trace::spans::child_message(
            name.as_deref().unwrap_or_default(),
            rustode_trace::spans::Direction::Incoming,
            "response",
        );
        let _entered = span.enter();
        tracing::trace!(%message, "from the server");
        let result = if message.get("error").is_some() {
            Value::Null
        } else {
            message["result"].clone()
        };
        match kind {
            Kind::Hover => self.hover_answered((server, request), &result),
            Kind::Completion => self.completion_answered((server, request), &result),
        }
        true
    }

    /// Forgets what was asked of `server`: it stopped or crashed, and will
    /// not answer.
    pub(super) fn lsp_forget_requests(&mut self, server: ServerId) {
        self.lsp.pending.retain(|(s, _), _| *s != server);
        if self
            .lsp
            .hover
            .as_ref()
            .is_some_and(|h| h.request.is_some_and(|(s, _)| s == server))
        {
            self.lsp.hover = None;
            self.events.push(Event::HoverChanged);
        }
        if let Some(session) = self.lsp.suggest.as_mut()
            && session.request.is_some_and(|(s, _)| s == server)
        {
            session.request = None;
        }
    }

    /// A document is closing: its hover and completion go away.
    pub(super) fn lsp_forget_document(&mut self, id: DocumentId) {
        self.hide_hover_of(id);
        self.dismiss_completion_of(id);
    }

    /// After a document's text changed: its hover is out of date, and its
    /// completion list is filtered again, or ends.
    pub(crate) fn lsp_edited(&mut self, id: DocumentId) {
        self.hide_hover_of(id);
        self.completion_edited(id);
    }
}
