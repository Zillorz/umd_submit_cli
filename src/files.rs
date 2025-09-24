// The code in this file crawls and packages the files to submit

use anyhow::{Context, Result};
use async_zip::{tokio::write::ZipFileWriter, ZipEntryBuilder, ZipString};
use indicatif::ProgressBar;
use regex::Regex;
use tokio::{fs::File, io::AsyncReadExt};
use walkdir::{DirEntry, WalkDir};

pub fn gen_paths() -> Result<Vec<String>> {
    println!("[3/5] Building regexes and walking directories...");
    let regexes = build_regexes()?;

    fn build_string(e: walkdir::Result<DirEntry>, regexes: &[Regex]) -> Option<String> {
        let path = e.ok()?.into_path();

        if path.is_dir() {
            return None;
        }

        let str = path.to_str()?;

        if str == "." {
            return None;
        };
        
        let new_str = str
            .trim_start_matches(r".\")
            .replace(r"\", "/");

        if is_included(regexes, &new_str) {
            Some(new_str)
        } else { None }
    }

    Ok(WalkDir::new(".").into_iter()
            .filter_map(|e| build_string(e, &regexes))
            .collect())
}

pub async fn pack(paths: &[String]) -> Result<Vec<u8>>  {
    println!("[4/5] Packing files...");
    let mut data = Vec::new();
    let mut writer = ZipFileWriter::with_tokio(&mut data);

    let bar = ProgressBar::new(paths.len() as u64);
    for path in paths {
        bar.set_message(format!("Packing {path}..."));
        let builder = ZipEntryBuilder::new(
            ZipString::from(path.as_ref()), async_zip::Compression::Deflate 
        );

        let mut file = File::open(path).await
            .with_context(|| format!("Could not open file '{path}'"))?;

        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).await
            .with_context(|| format!("Could not read file '{path}'"))?;

        writer.write_entry_whole(builder, &bytes).await
            .with_context(|| format!("Failed to add '{path}' to zip file"))?;

        bar.inc(1);
    }
    bar.finish_and_clear();

    writer.close()
        .await.context("Unable to write zip file")?;

    Ok(data)
}

static DEFAULT_FILTERS: [&str; 48] = [
      ".", "..", "core", "RCSLOG", "tags", "TAGS", "RCS", "SCCS", 
      ".make.state", ".nse_depinfo", 
      "#*", ".#*", "cvslog.*", ",*", ".git", "CVS", "CVS.adm", ".del-*", "*.a", 
      "*.olb", 
      "*.o", "*.obj", "*.so", "*.Z", "*~", "*.old", "*.elc", "*.ln", "*.bak", "*.BAK", 
      "*.orig", 
      "*.rej", "*.exe", "*.dll", "*.pdb", "*.lib", "*.ncb", "*.ilk", "*.exp", "*.suo", 
      ".DS_Store", "_$*", 
      "*$", "*.lo", "*.pch", "*.idb", "*.class", "~*"
];

fn generate_regex(filter: impl AsRef<str>) -> Result<Regex> {
    let filter = filter.as_ref()
        .replace("$", r"\$")
        .replace(".", r"\.")
        .replace("*", ".*");

    Regex::new(
        &format!(r"^(.*/)*{filter}")
    ).context("Failed to build regex, try deleting or redownloading '.submitIgnore'")
}

fn is_included(regexes: &[Regex], file: impl AsRef<str>) -> bool {
    regexes.iter()
        .all(|regex| !regex.is_match(file.as_ref()))
}

fn build_regexes() -> Result<Vec<Regex>> {
    let mut regexes = Vec::new();

    // do this at compile time?
    // nvm, writing a zip is SO much slower
    for filter in DEFAULT_FILTERS {
        regexes.push(
            generate_regex(filter)?
        )
    }

    Ok(regexes)
}
