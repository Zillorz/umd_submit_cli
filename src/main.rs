use std::{io::{self, Write}, path::Path, time::SystemTime};
use anyhow::{bail, Context, Result};
use colored::Colorize;
use reqwest::{multipart::{Form, Part}, Client};
use serde::{Serialize, Deserialize};
use tokio::{fs::File, io::{AsyncReadExt, AsyncWriteExt}};

mod files;

#[tokio::main]
async fn main() {
    if let Err(e) = run().await {
        eprintln!("{} {}", "[!]".bright_red(), e.to_string().bright_red()); 
    }
}

async fn run() -> Result<()> {
    let (config, user) = load_configs().await?; 

    if config.authentication != "cas" {
        bail!("Only CAS authentication is implemented, open a pull request!")
    }

    let user = match user {
        Some(a) => { 
            println!("[2/5] Loading user data...");
            a
        },
        None => generate_user(&config).await?
    };

    let files = files::gen_paths()?;
    let packed_files = files::pack(&files).await?;

    submit(config, user, packed_files).await?;

    Ok(())
}

async fn load_configs() -> Result<(SubmitConfig, Option<SubmitUser>)> {
    println!("[1/5] Loading configs...");
    let path = Path::new(".submit");
    
    let mut file = File::open(path)
        .await.context("Unable to open .submit file, does it exist?")?;

    let mut buffer = String::new();
    file.read_to_string(&mut buffer).await
        .context("Malformed .submit, try redownloading")?;
    
    let config: SubmitConfig = serde_java_properties::from_str(&buffer)
        .context("Malformed .submit, try redownloading")?;

    let path = Path::new(".submitUser");

    if !path.exists() {
        return Ok((config, None))
    }

    async fn try_submit_user() -> Result<SubmitUser> {
        let mut file = File::open(".submitUser")
            .await.inspect_err(|_| 
                eprintln!("{}", "[!] .submitUser was found, but was not opened".yellow())
            )?;

        let mut buffer = String::new();
            file.read_to_string(&mut buffer).await
            .inspect_err(|_| 
                eprintln!("{}", "[!] .submitUser was found, but was not readable".yellow())
            )?;
    
        Ok(serde_java_properties::from_str(&buffer) .inspect_err(|_| 
            eprintln!("{}", "[!] .submitUser was invalid, recreating...".yellow())
        )?)
    }

    if Path::new(".submitIgnore").exists() {
        eprintln!("{}", "[!] .submitIgnore is not currently implemented, may submit incorrectly".yellow());
    }

    if Path::new(".submitInclude").exists() {
        eprintln!("{}", "[!] .submitInclude is not currently implemented, may submit incorrectly".yellow());
    }
    
    Ok((config, try_submit_user().await.ok()))
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct SubmitConfig {
    #[allow(dead_code)]
    course_name: String,
    semester: String,
    project_number: String,
    course_key: String,
    #[serde(rename = "authentication.type")]
    authentication: String,
    #[serde(rename = "baseURL")]
    base_url: String,
    #[serde(rename = "submitURL")]
    submit_url: String
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct SubmitUser {
    class_account: String,
    one_time_password: String
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Params<'a> {
    course_key: &'a str,
    project_number: &'a str
}

async fn generate_user(config: &SubmitConfig) -> Result<SubmitUser> {
    println!("[2/5] Authenticating user...");

    let params = yaup::to_string(&Params {
        course_key: &config.course_key,
        project_number: &config.project_number
    }).context("Unable to format auth url")?;

    let url = format!("{}/view/submitStatus.jsp{}", config.base_url, params);
    if open::that_detached(&url).is_err() {
        println!("Cannot automatically open url");
        println!("Please open {url}");
    }

    print!("Paste here: ");
    io::stdout().flush()?;

    let mut buffer = String::new();
    io::stdin()
        .read_line(&mut buffer)
        .context("Cannot read from terminal (stdin), is it readable?")
        ?;

    let Some((ca, otp)) = buffer.trim().split_once(';') else {
        bail!("Invalid paste");
    };

    let user = SubmitUser {
        class_account: ca.to_string(),
        one_time_password: otp.to_string()
    };

    async fn try_save(su: &SubmitUser) -> Result<()> {
        let text = serde_java_properties::to_string(su)?;
    
        let mut file = File::create(".submitUser").await?;
        file.write_all(text.as_bytes()).await?;

        Ok(())
    }

    if try_save(&user).await.is_err() {
        eprintln!("Unable to save user, will ask again next time!");
    }
            
    Ok(user)
}

async fn submit(conf: SubmitConfig, user: SubmitUser, pack: Vec<u8>) -> Result<()> {
    let ms = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .expect("Time went backwards???")
        .as_millis();

    println!("[5/5] Submitting to server");
    let form = Form::new()
        .text("submitclientVersion", "0.3.1")
        // use proper timestamp in the future
        .text("cvstagTimestamp", format!("t{ms}"))
        .text("classAccount", user.class_account)
        .text("projectNumber", conf.project_number)
        .text("authentication.type", "cas")
        .text("oneTimePassword", user.one_time_password)
        .text("baseURL", conf.base_url)
        .text("semester", conf.semester)
        .text("courseKey", conf.course_key)
        // use proper failure in the future
        .text("hasFailedCVSOperation", "false")
        // pretend for now
        .text("submitClientTool", "EclipsePlugin")
        .part("submittedFiles", 
            Part::bytes(pack)
                .file_name("submit.zip")
        );

    let cli = Client::new();
    let res = cli.post(conf.submit_url)
        .multipart(form)
        .send().await
        .context("Failed to submit to server (http)")?;

    if res.status().is_success() {
        println!("{}", "Succesfully submitted project!".bright_green());
    } else {
        eprintln!("{} {}", "[!] Failed with http error: ".red(), res.status());
    }
    Ok(())
}

