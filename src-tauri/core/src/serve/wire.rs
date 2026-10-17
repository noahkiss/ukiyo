//! The socket protocol: newline-delimited JSON-RPC 2.0 carrying `api` requests.
//!
//! ```text
//! -> {"jsonrpc":"2.0","id":1,"method":"hello","params":{"actor":"app","protocol":1}}
//! <- {"jsonrpc":"2.0","id":1,"result":{"protocol":1,"version":"0.1.0","pid":4242,"fake":true}}
//! -> {"jsonrpc":"2.0","id":2,"method":"subscribe"}
//! <- {"jsonrpc":"2.0","id":2,"result":null}
//! <- {"jsonrpc":"2.0","method":"event","params":{"event":"status","data":{...}}}
//! -> {"jsonrpc":"2.0","id":3,"method":"plan_create","params":{"set":{"AutoPowerOff":"OFF"}}}
//! <- {"jsonrpc":"2.0","id":3,"result":{"plan_id":"p-7f3a","steps":[...],...}}
//! <- {"jsonrpc":"2.0","id":4,"error":{"code":"failed","message":"..."}}
//! ```
//!
//! - `method` and `params` are an `api::Request`; `result` is the `result` of the matching
//!   `api::Response` (`null` for a method with no result); `error` is an `api::Error`.
//! - `hello` and `subscribe` are the two methods outside `api`. `hello` is optional and names
//!   the actor; `subscribe` makes the connection receive every `api::Event` from then on.
//! - Replies may come back out of order (a `plan_apply` waits on the camera), so match by `id`.

use crate::api::{Error, ErrorCode, Event, Request, Response};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// Bumped when a change to the wire would break an older client or server.
pub const PROTOCOL: u32 = 1;

pub const HELLO: &str = "hello";
pub const SUBSCRIBE: &str = "subscribe";
/// The method name of an event notification.
pub const EVENT: &str = "event";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Hello {
    /// `app`, `cli` or `mcp:<client name>`.
    pub actor: String,
    pub protocol: u32,
}

/// The answer to `hello`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Welcome {
    pub protocol: u32,
    /// The serving binary's crate version.
    pub version: String,
    pub pid: u32,
    /// Whether this owner runs the fake camera.
    pub fake: bool,
}

/// One line a client sent, parsed.
#[derive(Debug)]
pub enum Incoming {
    Hello(Value, Hello),
    Subscribe(Value),
    Api(Value, Box<Request>),
    /// Not parseable: the id (when there was one) and why.
    Bad(Value, Error),
}

pub fn bad_request(message: impl Into<String>) -> Error {
    Error::new(ErrorCode::BadRequest, message)
}

pub fn unavailable(message: impl Into<String>) -> Error {
    Error::new(ErrorCode::Unavailable, message)
}

pub fn parse_incoming(line: &str) -> Incoming {
    let v: Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(e) => return Incoming::Bad(Value::Null, bad_request(format!("not JSON: {e}"))),
    };
    let id = v.get("id").cloned().unwrap_or(Value::Null);
    let Some(method) = v.get("method").and_then(Value::as_str) else {
        return Incoming::Bad(id, bad_request("no method"));
    };
    let params = v.get("params").cloned().filter(|p| !p.is_null());
    match method {
        HELLO => match serde_json::from_value(params.unwrap_or(Value::Null)) {
            Ok(h) => Incoming::Hello(id, h),
            Err(e) => Incoming::Bad(id, bad_request(format!("hello: {e}"))),
        },
        SUBSCRIBE => Incoming::Subscribe(id),
        _ => {
            let mut req = json!({ "method": method });
            if let Some(p) = params {
                req["params"] = p;
            }
            match serde_json::from_value::<Request>(req) {
                Ok(r) => Incoming::Api(id, Box::new(r)),
                Err(e) => Incoming::Bad(id, bad_request(format!("{method}: {e}"))),
            }
        }
    }
}

pub fn request_line(id: u64, method: &str, params: Option<Value>) -> String {
    let mut v = json!({ "jsonrpc": "2.0", "id": id, "method": method });
    if let Some(p) = params.filter(|p| !p.is_null()) {
        v["params"] = p;
    }
    v.to_string()
}

/// A request as a line; `api::Request` already has the `method` and `params` keys.
pub fn api_request_line(id: u64, req: &Request) -> String {
    let mut v = serde_json::to_value(req).unwrap_or(Value::Null);
    if let Some(o) = v.as_object_mut() {
        o.insert("jsonrpc".into(), json!("2.0"));
        o.insert("id".into(), json!(id));
    }
    v.to_string()
}

pub fn result_line(id: &Value, result: Value) -> String {
    json!({ "jsonrpc": "2.0", "id": id, "result": result }).to_string()
}

pub fn error_line(id: &Value, e: &Error) -> String {
    json!({ "jsonrpc": "2.0", "id": id, "error": e }).to_string()
}

/// The `result` part of a response.
pub fn response_result(r: &Response) -> Value {
    serde_json::to_value(r).ok().and_then(|mut v| v.get_mut("result").map(Value::take)).unwrap_or(Value::Null)
}

/// Rebuilds the `api::Response` for `method` from a reply's `result`.
pub fn response_from(method: &str, result: Value) -> Result<Response, Error> {
    let mut v = json!({ "method": method });
    if !result.is_null() {
        v["result"] = result;
    }
    serde_json::from_value(v).map_err(|e| bad_request(format!("reply to {method}: {e}")))
}

pub fn event_line(e: &Event) -> String {
    json!({ "jsonrpc": "2.0", "method": EVENT, "params": e }).to_string()
}

/// One line the server sent, parsed.
#[derive(Debug)]
pub enum Outgoing {
    Reply(u64, Result<Value, Error>),
    Event(Box<Event>),
    /// A line this client cannot use (an unknown event, a reply without an id).
    Other,
}

pub fn parse_outgoing(line: &str) -> Outgoing {
    let Ok(mut v) = serde_json::from_str::<Value>(line) else { return Outgoing::Other };
    if v.get("method").and_then(Value::as_str) == Some(EVENT) {
        return match v.get_mut("params").map(Value::take).map(serde_json::from_value::<Event>) {
            Some(Ok(e)) => Outgoing::Event(Box::new(e)),
            _ => Outgoing::Other,
        };
    }
    let Some(id) = v.get("id").and_then(Value::as_u64) else { return Outgoing::Other };
    if let Some(e) = v.get_mut("error").map(Value::take) {
        let e = serde_json::from_value(e.clone()).unwrap_or_else(|_| bad_request(e.to_string()));
        return Outgoing::Reply(id, Err(e));
    }
    Outgoing::Reply(id, Ok(v.get_mut("result").map(Value::take).unwrap_or(Value::Null)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::camera::owner::Status;

    #[test]
    fn requests_round_trip() {
        let line = api_request_line(7, &Request::PlanApply { plan_id: "p-1".into() });
        let Incoming::Api(id, r) = parse_incoming(&line) else { panic!("{line}") };
        assert_eq!(id, json!(7));
        assert!(matches!(*r, Request::PlanApply { ref plan_id } if plan_id == "p-1"));
        let Incoming::Api(_, r) = parse_incoming(&api_request_line(8, &Request::LabSnapshot)) else { panic!() };
        assert!(matches!(*r, Request::LabSnapshot));
        let Incoming::Hello(_, h) = parse_incoming(&request_line(1, HELLO, Some(json!({ "actor": "cli", "protocol": 1 })))) else { panic!() };
        assert_eq!(h.actor, "cli");
        assert!(matches!(parse_incoming(&request_line(2, SUBSCRIBE, None)), Incoming::Subscribe(_)));
        let Incoming::Bad(id, e) = parse_incoming(r#"{"id":3,"method":"lab_reset_usb"}"#) else { panic!() };
        assert_eq!((id, e.code), (json!(3), ErrorCode::BadRequest));
        assert!(matches!(parse_incoming("{"), Incoming::Bad(Value::Null, _)));
    }

    #[test]
    fn responses_round_trip() {
        for r in [
            Response::LabPin,
            Response::LabExport("/x.json".into()),
            Response::PlanDiscard,
        ] {
            let method = serde_json::to_value(&r).unwrap()["method"].as_str().unwrap().to_string();
            let line = result_line(&json!(5), response_result(&r));
            let Outgoing::Reply(5, Ok(v)) = parse_outgoing(&line) else { panic!("{line}") };
            let back = response_from(&method, v).unwrap();
            assert_eq!(serde_json::to_value(&back).unwrap(), serde_json::to_value(&r).unwrap());
        }
        let e = Error::new(ErrorCode::PlanStale, "no").hint("again");
        let Outgoing::Reply(9, Err(back)) = parse_outgoing(&error_line(&json!(9), &e)) else { panic!() };
        assert_eq!(back, e);
        let Outgoing::Event(ev) = parse_outgoing(&event_line(&Event::Status(Status::default()))) else { panic!() };
        assert!(matches!(*ev, Event::Status(_)));
    }
}
