use futures::Async::*;
use futures::{Poll, Stream};
use std::collections::VecDeque;
use std::fmt;
use std::fmt::Display;
use std::vec::Vec;

#[derive(Debug)]
pub enum LogMergeError {
    DefaultError,
}

impl Display for LogMergeError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Failed stream.")
    }
}

pub type LogStream = Box<Stream<Item = String, Error = LogMergeError>>;

#[derive(PartialEq)]
enum SourceState {
    NeedsPoll,
    Delivered,
    Finished,
}

struct LogLine {
    source_idx: usize,
    line: String,
}

pub struct LogMerge {
    sources: Vec<LogStream>,
    source_state: Vec<SourceState>,
    finished: usize,
    buffer: VecDeque<LogLine>,
}

impl LogMerge {
    pub fn new(sources: Vec<LogStream>) -> LogMerge {
        let num_sources = sources.len();
        let mut source_state = Vec::with_capacity(sources.len());
        for _ in 0..sources.len() {
            source_state.push(SourceState::NeedsPoll);
        }
        LogMerge {
            sources: sources,
            source_state: source_state,
            finished: 0,
            buffer: VecDeque::with_capacity(num_sources),
        }
    }

    fn state(&self) -> SourceState {
        // println!("finished: {} of {}", self.finished, self.sources.len());
        let unfinished = self.sources.len() - self.finished;
        if unfinished == 0 {
            SourceState::Finished
        } else if unfinished == self.buffer.len() {
            SourceState::Delivered
        } else {
            SourceState::NeedsPoll
        }
    }

    fn next_line(&mut self) -> LogLine {
        self.buffer.pop_front().unwrap()
    }

    fn insert_into_buffer(&mut self, line: LogLine) {
        self.buffer.push_back(line);
    }

    fn poll_source(&mut self, source_idx: usize) -> Result<(), LogMergeError> {
        match self.sources[source_idx].poll() {
            Ok(Ready(Some(line))) => {
                let log_line = LogLine { source_idx, line };
                self.insert_into_buffer(log_line);
                self.source_state[source_idx] = SourceState::Delivered;
            }
            Ok(Ready(None)) => {
                self.source_state[source_idx] = SourceState::Finished;
                self.finished += 1;
            }
            Ok(NotReady) => {
                self.source_state[source_idx] = SourceState::NeedsPoll;
            }
            Err(_) => {
                return Err(LogMergeError::DefaultError);
            }
        }
        Ok(())
    }
}

impl Stream for LogMerge {
    type Item = String;
    type Error = LogMergeError;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        // println!("poll");
        for s in 0..self.source_state.len() {
            match self.source_state[s] {
                SourceState::NeedsPoll => {
                    if let Err(err) = self.poll_source(s) {
                        return Err(err);
                    }
                }
                _ => {}
            }
        }
        match self.state() {
            SourceState::Delivered => {
                let log_line = self.next_line();
                self.source_state[log_line.source_idx] = SourceState::NeedsPoll;
                Ok(Ready(Some(log_line.line)))
            }
            SourceState::Finished => Ok(Ready(None)),
            SourceState::NeedsPoll => Ok(NotReady),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::log_merge::{LogMerge, LogStream};
    use futures::stream::{empty, iter_ok, once};
    use futures::Stream;
    use tokio::runtime::current_thread::Runtime;

    #[test]
    fn test_new() {
        let s1: LogStream = Box::new(once(Ok(String::from("s1"))));
        let s2: LogStream = Box::new(once(Ok(String::from("s2"))));
        let sources = vec![s1, s2];
        let merge = LogMerge::new(sources);
        assert!(merge.sources.len() == 2);
        assert!(merge.source_state.len() == 2);
    }

    #[test]
    fn empty_streams() {
        let s1: LogStream = Box::new(empty());
        let sources = vec![s1];
        let merge = LogMerge::new(sources);
        let mut rt = Runtime::new().unwrap();
        let result = rt.block_on(merge.collect()).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_single_stream() {
        let s1: LogStream = Box::new(iter_ok(vec![String::from("s11"), String::from("s12")]));
        let sources = vec![s1];
        let merge = LogMerge::new(sources);
        let mut rt = Runtime::new().unwrap();
        let result = rt.block_on(merge.collect()).unwrap();
        assert_eq!(vec![String::from("s11"), String::from("s12")], result);
    }

    #[test]
    fn test_multiple_streams_same_length() {
        let s1: LogStream = Box::new(iter_ok(vec![String::from("s11"), String::from("s12")]));
        let s2: LogStream = Box::new(iter_ok(vec![String::from("s21"), String::from("s22")]));
        let s3: LogStream = Box::new(iter_ok(vec![String::from("s31"), String::from("s32")]));
        let sources = vec![s1, s2, s3];
        let merge = LogMerge::new(sources);
        let mut rt = Runtime::new().unwrap();
        let result = rt.block_on(merge.collect()).unwrap();
        assert_eq!(
            vec![
                String::from("s11"),
                String::from("s21"),
                String::from("s31"),
                String::from("s12"),
                String::from("s22"),
                String::from("s32")
            ],
            result
        );
    }

    #[test]
    fn test_multiple_streams_varying_length() {
        let s1: LogStream = Box::new(iter_ok(vec![String::from("s11"), String::from("s12")]));
        let s2: LogStream = Box::new(iter_ok(vec![String::from("s21")]));
        let s3: LogStream = Box::new(iter_ok(vec![
            String::from("s31"),
            String::from("s32"),
            String::from("s33"),
        ]));
        let sources = vec![s1, s2, s3];
        let merge = LogMerge::new(sources);
        let mut rt = Runtime::new().unwrap();
        let result = rt.block_on(merge.collect()).unwrap();
        assert_eq!(
            vec![
                String::from("s11"),
                String::from("s21"),
                String::from("s31"),
                String::from("s12"),
                String::from("s32"),
                String::from("s33")
            ],
            result
        );
    }
}
