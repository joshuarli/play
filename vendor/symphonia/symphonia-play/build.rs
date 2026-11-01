// Symphonia
// Copyright (c) 2019-2026 The Project Symphonia Developers.
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

const GIT_SHA_ENV: &str = "SYMPHONIA_PLAY_GIT_SHA";

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed={GIT_SHA_ENV}");

    let manifest_dir = PathBuf::from(
        env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is set by Cargo"),
    );

    let sha = match env::var_os(GIT_SHA_ENV).filter(|value| !value.is_empty()) {
        Some(sha) => sha.into_string().unwrap_or_else(|_| {
            panic!("{GIT_SHA_ENV} must be a hexadecimal commit SHA encoded as UTF-8")
        }),
        // Source archives without .git metadata must set the override. Failing here is
        // intentional: a build must never embed a placeholder commit SHA.
        None => git_sha(&manifest_dir),
    };

    let sha = sha.trim().to_owned();
    validate_sha(&sha);

    println!("cargo:rustc-env={GIT_SHA_ENV}={sha}");
}

fn git_sha(manifest_dir: &Path) -> String {
    let git_dir = git_output(manifest_dir, &["rev-parse", "--git-dir"]).unwrap_or_else(|| {
        panic!(
            "could not determine the git commit SHA; set {GIT_SHA_ENV} to a hexadecimal SHA \
             when building from a source archive without .git metadata"
        )
    });
    let git_dir = resolve_git_path(manifest_dir, &git_dir);

    rerun_if_changed(&git_dir.join("HEAD"));
    rerun_if_changed(&git_dir.join("index"));

    let common_dir = git_output(manifest_dir, &["rev-parse", "--git-common-dir"])
        .map(|dir| resolve_git_path(manifest_dir, &dir))
        .unwrap_or_else(|| git_dir.clone());

    rerun_if_changed(&common_dir.join("refs"));
    rerun_if_changed(&common_dir.join("packed-refs"));

    git_output(manifest_dir, &["rev-parse", "--short=12", "HEAD"]).unwrap_or_else(|| {
        panic!(
            "could not read the git commit SHA; set {GIT_SHA_ENV} to a hexadecimal SHA \
             when building from a source archive without .git metadata"
        )
    })
}

fn git_output(manifest_dir: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git").arg("-C").arg(manifest_dir).args(args).output().ok()?;

    if !output.status.success() {
        return None;
    }

    String::from_utf8(output.stdout).ok().map(|stdout| stdout.trim().to_owned()).filter(|stdout| {
        !stdout.is_empty()
    })
}

fn resolve_git_path(manifest_dir: &Path, path: &str) -> PathBuf {
    let path = PathBuf::from(path);

    if path.is_absolute() { path } else { manifest_dir.join(path) }
}

fn rerun_if_changed(path: &Path) {
    if path.exists() {
        println!("cargo:rerun-if-changed={}", path.display());
    }
}

fn validate_sha(sha: &str) {
    let is_placeholder = sha.eq_ignore_ascii_case("unknown") || sha.eq_ignore_ascii_case("dirty");
    let is_hexadecimal = sha.bytes().all(|byte| byte.is_ascii_hexdigit());

    if sha.is_empty() || is_placeholder || !is_hexadecimal {
        panic!(
            "git SHA '{sha}' is not a hexadecimal commit SHA; set {GIT_SHA_ENV} to a real \
             hexadecimal SHA"
        );
    }
}
