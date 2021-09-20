use super::{find_tag, Packet, Tag};
use lazy_static::lazy_static;
use regex::Regex;

lazy_static! {
    static ref TAG_PATTERN: Regex = Regex::new(r"^([^:]*): *(.*)$").unwrap();
}

#[derive(Debug)]
pub enum Response {
    CommandResponse(Vec<Packet>),
    Event(Packet),
}

pub struct ResponseBuilder {
    response: Vec<Packet>,
    in_packet: Packet,
    in_response_sequence: bool,
}

impl ResponseBuilder {
    pub fn new() -> ResponseBuilder {
        Self {
            response: vec![],
            in_packet: vec![],
            in_response_sequence: false,
        }
    }

    /// processes a single line received from the Asterisk server
    ///
    /// # Arguments
    ///
    /// * `line` - a line that has been read from the server connection (must not include terminating line break)
    ///
    /// Returns `None` if neither a response nor an event is complete, `Some(...)` if a response
    /// is complete.
    pub fn add_line(&mut self, line: &str) -> Option<Response> {
        if line.is_empty() {
            if !self.in_response_sequence
                && !self.in_packet.is_empty()
                && self.in_packet[0].key.eq_ignore_ascii_case("Event")
            {
                let data = self.in_packet.clone();
                self.in_packet.clear();
                return Some(Response::Event(data));
            } else {
                self.response.push(self.in_packet.clone());
                let event_list =
                    find_tag(&self.in_packet, "EventList").cloned();
                self.in_packet.clear();
                if let Some(el_val) = event_list {
                    if el_val.eq_ignore_ascii_case("start") {
                        self.in_response_sequence = true;
                    } else if el_val.eq_ignore_ascii_case("Complete") {
                        self.in_response_sequence = false;
                    }
                }
                if !self.in_response_sequence {
                    let data = self.response.clone();
                    self.response.clear();
                    return Some(Response::CommandResponse(data));
                }
            }
        } else {
            if let Some(tag) = line_to_tag(line) {
                self.in_packet.push(tag);
            }
        }

        None
    }
}

fn line_to_tag(line: &str) -> Option<Tag> {
    let caps = TAG_PATTERN.captures(line)?;
    Some(Tag::from(&caps[1], &caps[2]))
}
