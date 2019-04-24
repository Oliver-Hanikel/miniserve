use assert_cmd::prelude::*;
use assert_fs::fixture::TempDir;
use assert_fs::prelude::*;
use clap::{crate_name, crate_version};
use port_check::free_local_port;
use pretty_assertions::assert_eq;
use reqwest;
use reqwest::multipart;
use select::document::Document;
use select::predicate::{Attr, Text};
use std::process::{Command, Stdio};
use std::thread::sleep;
use std::time::Duration;

type Error = Box<std::error::Error>;

static FILES: &[&str] = &["test.txt", "test.html", "test.mkv"];

/// Test fixture which creates a temporary directory with a few files inside.
pub fn tmpdir() -> Result<TempDir, Error> {
    let tmpdir = assert_fs::TempDir::new()?;
    for &file in FILES {
        tmpdir.child(file).write_str("Test Hello Yes")?;
    }
    Ok(tmpdir)
}

#[test]
fn serves_requests_with_no_options() -> Result<(), Error> {
    let tmpdir = tmpdir()?;
    let mut child = Command::cargo_bin("miniserve")?
        .arg(tmpdir.path())
        .stdout(Stdio::null())
        .spawn()?;

    sleep(Duration::from_secs(1));

    let body = reqwest::get("http://localhost:8080")?.error_for_status()?;
    let parsed = Document::from_read(body)?;
    for &file in FILES {
        assert!(parsed.find(Text).any(|x| x.text() == file));
    }

    child.kill()?;

    Ok(())
}

#[test]
fn serves_requests_with_non_default_port() -> Result<(), Error> {
    let tmpdir = tmpdir()?;

    let port = free_local_port().unwrap();
    let mut child = Command::cargo_bin("miniserve")?
        .arg(tmpdir.path())
        .arg("-p")
        .arg(port.to_string())
        .stdout(Stdio::null())
        .spawn()?;

    sleep(Duration::from_secs(1));

    let body = reqwest::get(format!("http://localhost:{}", port).as_str())?.error_for_status()?;
    let parsed = Document::from_read(body)?;
    for &file in FILES {
        assert!(parsed.find(Text).any(|x| x.text() == file));
    }

    child.kill()?;

    Ok(())
}

#[test]
fn auth_works() -> Result<(), Error> {
    let tmpdir = tmpdir()?;

    let port = free_local_port().unwrap();
    let mut child = Command::cargo_bin("miniserve")?
        .arg(tmpdir.path())
        .arg("-p")
        .arg(port.to_string())
        .arg("-a")
        .arg("testuser:testpassword")
        .stdout(Stdio::null())
        .spawn()?;

    sleep(Duration::from_secs(1));

    let client = reqwest::Client::new();
    let body = client
        .get(format!("http://localhost:{}", port).as_str())
        .basic_auth("testuser", Some("testpassword"))
        .send()?
        .error_for_status()?;
    let parsed = Document::from_read(body)?;
    for &file in FILES {
        assert!(parsed.find(Text).any(|x| x.text() == file));
    }

    child.kill()?;

    Ok(())
}

#[test]
fn uploading_files_works() -> Result<(), Error> {
    let tmpdir = tmpdir()?;

    let test_file_name = "uploaded test file.txt";

    let port = free_local_port().unwrap();
    let mut child = Command::cargo_bin("miniserve")?
        .arg(tmpdir.path())
        .arg("-p")
        .arg(port.to_string())
        .arg("-u")
        .stdout(Stdio::null())
        .spawn()?;

    sleep(Duration::from_secs(1));

    // Before uploading, check whether the uploaded file does not yet exist.
    let body = reqwest::get(format!("http://localhost:{}", port).as_str())?.error_for_status()?;
    let parsed = Document::from_read(body)?;
    assert!(parsed.find(Text).all(|x| x.text() != test_file_name));

    // Perform the actual upload.
    let upload_action = parsed
        .find(Attr("id", "file_submit"))
        .next()
        .expect("Couldn't find element with id=file_submit")
        .attr("action")
        .expect("Upload form doesn't have action attribute");
    let form = multipart::Form::new();
    let part = multipart::Part::text("this should be uploaded")
        .file_name(test_file_name)
        .mime_str("text/plain")?;
    let form = form.part("file_to_upload", part);

    let client = reqwest::Client::new();
    client
        .post(format!("http://localhost:{}{}", port, upload_action).as_str())
        .multipart(form)
        .send()?
        .error_for_status()?;

    // After uploading, check whether the uploaded file is now getting listed.
    let body = reqwest::get(format!("http://localhost:{}", port).as_str())?;
    let parsed = Document::from_read(body)?;
    assert!(parsed.find(Text).any(|x| x.text() == test_file_name));

    child.kill()?;

    Ok(())
}

#[test]
/// Show help and exit.
fn help_shows() -> Result<(), Error> {
    Command::cargo_bin("miniserve")?
        .arg("-h")
        .assert()
        .success();

    Ok(())
}

#[test]
/// Show version and exit.
fn version_shows() -> Result<(), Error> {
    Command::cargo_bin("miniserve")?
        .arg("-V")
        .assert()
        .success()
        .stdout(format!("{} {}\n", crate_name!(), crate_version!()));

    Ok(())
}
