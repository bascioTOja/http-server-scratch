use std::io::{Read, Write};
use std::net::TcpListener;
use std::net::TcpStream;
use std::{env, thread};
use std::fs::{read, File};
use std::path::{Path, PathBuf};
use flate2::Compression;
use flate2::write::GzEncoder;

fn get_buffer(mut stream: &TcpStream) -> Vec<u8> {
    let mut buffer = [0; 1024];
    let buffer_size = stream.read(&mut buffer).unwrap();

    buffer[..buffer_size].to_vec()
}

fn get_request_start_line(request_line: &mut std::str::Split<&str>) -> StartLine {
    let mut parts = request_line.next().unwrap_or("").split_whitespace();

    StartLine {
        method: parts.next().unwrap_or("").to_string(),
        target: parts.next().unwrap_or("").to_string(),
        version: parts.next().unwrap_or("").to_string(),
    }
}

fn get_request_headers(request_lines: &mut std::str::Split<&str>) -> Headers {
    let mut host: String = String::new();
    let mut user_agent: String = String::new();
    let mut accept: String = String::new();
    let mut accept_encoding: Vec<String> = Vec::new();
    let mut content_type: String = String::new();
    let mut content_length: String = String::new();

    for line in request_lines {
        if line.starts_with("Host: ") {
            host = line[6..].to_string();
        } else if line.starts_with("User-Agent: ") {
            user_agent = line[12..].to_string();
        } else if line.starts_with("Accept: ") {
            accept = line[8..].to_string();
        }  else if line.starts_with("Accept-Encoding: ") {
            let mut encodings = line[17..].to_string().split(",").map(|s| s.trim().to_string()).collect::<Vec<String>>();
            accept_encoding.append(&mut encodings);
        } else if line.starts_with("Content-Type: ") {
            content_type = line[14..].to_string();
        } else if line.starts_with("Content-Length: ") {
            content_length = line[16..].to_string();
        } else if line == "" {
            break;
        }
    }

    Headers {
        host,
        user_agent,
        accept,
        accept_encoding,
        content_type,
        content_length,
    }
}

fn get_file_response_format(path: &Path) -> String {
    let extension = path.extension().unwrap_or(std::ffi::OsStr::new(""));

    if extension == "html" {
        return String::from("text/html");
    } else if extension == "js" {
        return String::from("text/javascript");
    } else if extension == "css" {
        return String::from("text/css");
    }  else if extension == "txt" {
        return String::from("text/plain");
    } else if extension == "jpg" || extension == "jpeg" {
        return String::from("image/jpeg");
    } else if extension == "png" {
        return String::from("image/png");
    }

    return String::from("application/octet-stream");
}

fn controller(request: &Request, response: &mut Response, files_path: &Path) {
    if request.start.target == "/" {
        response.status = String::from("200 OK");
        response.format = String::from("text/plain");

        return;
    }

    if request.start.target == "/user-agent" && request.start.method == "GET" {
        response.status = String::from("200 OK");
        response.format = String::from("text/plain");
        response.body = request.headers.user_agent.clone().to_string().as_bytes().to_vec();

        return;
    }

    if request.start.method == "GET" && request.start.target.starts_with("/echo/") {
        let echo_message = &request.start.target[6..];
        response.status = String::from("200 OK");
        response.format = String::from("text/plain");
        response.body = echo_message.to_string().as_bytes().to_vec();

        return;
    }

    if request.start.method == "GET" && (request.start.target == "/files" || request.start.target.starts_with("/files/")) {
        let file_path: PathBuf;
        if request.start.target == "/files" {
            file_path = files_path.join(&request.start.target[6..]).join("index.html");
        } else {
            file_path = files_path.join(&request.start.target[7..]);
        }

        println!("{}", file_path.display());

        if file_path.exists() && file_path.is_file() {
            response.status = String::from("200 OK");
            response.format = get_file_response_format(&file_path);
            response.body = read(file_path).unwrap().to_vec();

            return;
        }
    }

    if request.start.method == "POST" && request.headers.content_type == "application/octet-stream" && request.start.target.starts_with("/files/") {
        let file_path = files_path.join(&request.start.target[7..]);
        println!("{}", file_path.display());

        let mut file = File::create(file_path).unwrap();
        file.write_all(request.body.as_bytes()).unwrap();

        response.status = String::from("201 Created");

        return;
    }

    response.status = String::from("404 Not Found");
    response.format = String::from("text/plain");

    return;
}

struct StartLine {
    method: String,
    target: String,
    version: String,
}

struct Headers {
    host: String,
    user_agent: String,
    accept: String,
    accept_encoding: Vec<String>,
    content_type: String,
    content_length: String,
}

struct Request {
    start: StartLine,
    headers: Headers,
    body: String,
}

struct Response {
    version: String,
    status: String,
    format: String,
    content_encoding: String,
    body: Vec<u8>,
}

impl Response {
    fn build(&self) -> String {
        let mut headers = format!("{} {}\r\nContent-Type: {}\r\n", self.version, self.status, self.format);

        if self.content_encoding != "" {
            headers.push_str(&format!("Content-Encoding: {}\r\n", self.content_encoding));
        }
        headers.push_str(&format!("Content-Length: {}\r\n\r\n", self.body.len()));

        headers
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let directory: String;
    if args.len() >= 3 && args[1] == "--directory" {
        directory = args[2].clone();
    } else {
        directory = env::current_dir().unwrap().join(PathBuf::from("files")).to_str().unwrap().to_string();
    }

    println!("Files path: {}", directory);

    let listener = TcpListener::bind("127.0.0.1:8080").unwrap();
    for stream in listener.incoming() {
        let stream = stream.unwrap();
        let directory = directory.clone();

        thread::spawn(|| {
            handle_connection(stream, directory);
        });
    }
}

fn handle_connection(mut stream: TcpStream, directory: String) {
    println!("accepted new connection from {}", stream.peer_addr().unwrap());

    let buffer = get_buffer(&stream);
    let request_raw = String::from_utf8_lossy(buffer.as_slice());
    println!("Request: {:?}", &request_raw);

    let mut request_lines = request_raw.split("\r\n");

    let request = Request {
        start: get_request_start_line(&mut request_lines),
        headers: get_request_headers(&mut request_lines),
        body: request_raw[request_raw.find("\r\n\r\n").unwrap_or(request_raw.len()) + 4..].to_string(),
    };

    println!("Start: {} {} {}", &request.start.method, &request.start.target, &request.start.version);
    println!("Headers: {} {} {} {} {}", &request.headers.host, &request.headers.user_agent, &request.headers.accept, &request.headers.content_type, &request.headers.content_length);

    let mut response = Response {
        version: String::from("HTTP/1.1"),
        status: String::new(),
        format: String::new(),
        content_encoding: String::new(),
        body: Vec::new(),
    };

    controller(&request, &mut response, Path::new(&directory));

    if request.headers.accept_encoding.contains(&String::from("gzip")) {
        response.content_encoding = String::from("gzip");
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(&response.body).unwrap();
        response.body = encoder.finish().unwrap();
    }

    stream.write_all(response.build().as_bytes()).unwrap();
    stream.write_all(&response.body).unwrap();
}
