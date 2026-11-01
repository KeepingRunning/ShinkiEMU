use super::*;

/// GitHub issue forms use headings, while script text may itself contain headings.
/// Keep fenced code intact and reject ambiguous duplicate form sections.
fn form(body: &str) -> Result<BTreeMap<String, String>> {
    ensure!(body.len() <= 64 * 1024, "issue body too large");
    let body = body.replace("\r\n", "\n");
    let mut result = BTreeMap::new();
    let mut heading = None;
    let mut lines = Vec::new();
    let mut fence: Option<(char, usize)> = None;
    let save = |heading: Option<String>, lines: &mut Vec<&str>, result: &mut BTreeMap<String, String>| -> Result<()> {
        if let Some(key) = heading {
            let value = lines.join("\n").trim().to_owned();
            ensure!(!result.contains_key(&key), "duplicate form section: {key}");
            result.insert(key, if value == "_No response_" { String::new() } else { value });
        }
        lines.clear();
        Ok(())
    };
    for line in body.lines() {
        let trimmed = line.trim_start();
        let marker = trimmed.chars().next().filter(|c| matches!(c, '`' | '~'));
        if let Some(marker) = marker {
            let count = trimmed.chars().take_while(|c| *c == marker).count();
            if count >= 3 {
                match fence {
                    None => fence = Some((marker, count)),
                    Some((open, length)) if marker == open && count >= length
                        && trimmed[count..].trim().is_empty() => fence = None,
                    _ => {}
                }
                if heading.is_some() { lines.push(line); }
                continue;
            }
        }
        if fence.is_none() && let Some(key) = line.strip_prefix("### ") {
            save(heading.take(), &mut lines, &mut result)?;
            heading = Some(key.trim().to_owned());
        } else if heading.is_some() {
            lines.push(line);
        }
    }
    ensure!(fence.is_none(), "unclosed form code fence");
    save(heading, &mut lines, &mut result)?;
    Ok(result)
}

fn unfence(value: &str) -> Result<&str> {
    let Some((first, rest)) = value.split_once('\n') else { return Ok(value); };
    let first = first.trim();
    if !first.starts_with("```") && !first.starts_with("~~~") { return Ok(value); }
    let marker = first.as_bytes()[0] as char;
    let length = first.chars().take_while(|c| *c == marker).count();
    let (content, last) = rest.rsplit_once('\n').context("missing closing code fence")?;
    let last = last.trim();
    ensure!(last.len() >= length && last.chars().all(|c| c == marker), "invalid closing code fence");
    Ok(content)
}

fn required(fields: &BTreeMap<String, String>, key: &str) -> Result<String> {
    fields.get(key).filter(|value| !value.is_empty()).cloned()
        .with_context(|| format!("missing form field: {key}"))
}

fn json_field<T: DeserializeOwned>(fields: &BTreeMap<String, String>, key: &str) -> Result<T> {
    let text = required(fields, key)?;
    let value = parse_unique_json(unfence(&text)?.as_bytes())
        .with_context(|| format!("invalid JSON in form field: {key}"))?;
    serde_json::from_value(value).with_context(|| format!("invalid form field: {key}"))
}

fn compatibility(fields: &BTreeMap<String, String>, number: u64, timestamp: i64) -> Result<(String, Vec<u8>)> {
    let entry = CompatibilityEntry {
        vn_id: required(fields, "VNDB vID")?,
        release_id: required(fields, "VNDB rID")?,
        emulator: required(fields, "Core")?,
        status: serde_json::from_value(Value::String(required(fields, "Status")?))?,
        notes: fields.get("Notes / reproduction").filter(|value| !value.is_empty()).cloned(),
        emulator_version: Some(required(fields, "Core version")?),
        manager_version: Some(required(fields, "Manager version")?),
        platform: required(fields, "Platform")?,
        architecture: required(fields, "Architecture")?,
        variant: required(fields, "Variant")?,
        issue: Some(number),
        updated_at_unix_ms: timestamp,
    };
    entry.validate()?;
    Ok((compat_path(&entry), pretty(&entry)?))
}

fn downloads(fields: &BTreeMap<String, String>) -> Result<Vec<PatchDownload>> {
    let downloads: Vec<PatchDownload> = json_field(fields, "Downloads (JSON)")?;
    ensure!(downloads.len() <= 64, "too many download declarations");
    let mut urls = BTreeSet::new();
    for (index, item) in downloads.iter().enumerate() {
        let url = reqwest::Url::parse(&item.url)
            .with_context(|| format!("download {index}: invalid HTTPS URL"))?;
        ensure!(url.scheme() == "https" && url.host_str().is_some()
            && url.username().is_empty() && url.password().is_none()
            && url.fragment().is_none(), "download {index}: URL must be HTTPS without credentials or fragment");
        ensure!(item.sha256.len() == 64 && item.sha256.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
            "download {index}: sha256 must contain 64 lowercase hex characters");
        ensure!(item.size <= i64::MAX as u64, "download {index}: size exceeds VM integer range");
        ensure!(urls.insert(&item.url), "duplicate download URL: {}", item.url);
    }
    Ok(downloads)
}

fn patch(fields: &BTreeMap<String, String>, number: u64) -> Result<BTreeMap<String, Vec<u8>>> {
    let id = required(fields, "Patch id")?;
    ensure!(is_safe_metadata_relative_path(&id) && !id.contains('/'), "invalid Patch id");
    let releases: Vec<PatchRelease> = json_field(fields, "Release bindings (JSON)")?;
    for release in &releases {
        ensure!(release.release_id.len() <= 64 && release.release_id.strip_prefix('r')
            .is_some_and(|id| !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit())),
            "invalid VNDB rID: {}", release.release_id);
        ensure!(release.vn_id.len() <= 64 && release.vn_id.strip_prefix('v')
            .is_some_and(|id| !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit())),
            "invalid VNDB vID: {}", release.vn_id);
        release.validate().context("invalid release binding or source file fingerprint")?;
    }
    let downloads = downloads(fields)?;
    let entry = PatchCatalogEntry {
        id: id.clone(),
        title: BTreeMap::from([("zh-CN".into(), required(fields, "Title (zh-CN)")?), ("en".into(), required(fields, "Title (en)")?)]),
        version: required(fields, "Version")?,
        emulators: required(fields, "Cores")?.split([',', '\n']).map(str::trim)
            .filter(|value| !value.is_empty()).map(str::to_owned).collect(),
        source_url: Some(required(fields, "Source URL")?),
        license: required(fields, "License")?,
        authorization_url: fields.get("Authorization URL").filter(|value| !value.is_empty()).cloned(),
        summary: BTreeMap::from([("zh-CN".into(), required(fields, "Summary (zh-CN)")?), ("en".into(), required(fields, "Summary (en)")?)]),
        issue_url: Some(format!("https://github.com/Project-Shinkirou/ShinkiEMU/issues/{number}")),
        recipe: PatchRecipe { script: format!("{id}.luau") },
        releases,
        downloads,
    };
    entry.validate().context("invalid patch catalog entry")?;
    let text = required(fields, "Luau script")?;
    let script = unfence(&text)?;
    ensure!(!script.trim().is_empty(), "missing Luau script text");
    let temporary = tempfile::tempdir()?;
    let script_path = temporary.path().join("entry.luau");
    fs::write(&script_path, script)?;
    shinki_emu_patch::check_syntax(&script_path).context("Luau syntax check failed")?;
    Ok(BTreeMap::from([
        (format!("patches/entries/{id}.json"), pretty(&entry)?),
        (format!("patches/{}", entry.recipe.script), script.as_bytes().to_vec()),
    ]))
}

pub(super) fn intake(kind: IntakeKind, event: &Path, out: &Path) -> Result<()> {
    let event: Value = parse_unique_json(&read(event)?)?;
    let issue = event.get("issue").context("missing issue")?;
    let body = issue["body"].as_str().context("missing body")?;
    let number = issue["number"].as_u64().filter(|n| *n > 0).context("invalid issue number")?;
    let updated = issue["updated_at"].as_str().context("missing updated_at")?;
    let timestamp = chrono::DateTime::parse_from_rfc3339(updated)?.timestamp_millis();
    ensure!(timestamp >= 0, "negative timestamp");
    let fields = form(body)?;
    let (path, files) = match kind {
        IntakeKind::Compatibility => {
            let (path, bytes) = compatibility(&fields, number, timestamp)?;
            (path.clone(), BTreeMap::from([(path, bytes)]))
        }
        IntakeKind::Patch => {
            let files = patch(&fields, number)?;
            let path = format!("patches/entries/{}.json", required(&fields, "Patch id")?);
            (path, files)
        }
    };
    let paths: Vec<_> = files.keys().cloned().collect();
    let binding = json!({"issue":number,"body_sha256":digest(body.as_bytes()),"updated_at":updated,"path":path,"paths":paths});
    let mut files: BTreeMap<_, _> = files.into_iter().map(|(path, bytes)| (format!("Metadata/{path}"), bytes)).collect();
    files.insert("approval.json".into(), pretty(&binding)?);
    write_new_tree(out, &files)?;
    println!("{}", serde_json::to_string(&binding)?);
    Ok(())
}

#[cfg(test)]
mod tests;
