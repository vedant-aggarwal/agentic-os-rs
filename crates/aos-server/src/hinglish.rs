//! Roman-script Hinglish for voice transcripts.
//!
//! Codex dictation writes the Hindi part of a Hinglish take in Devanagari, but the app's text is
//! typed Hinglish in Roman script. `apply` rewrites such a transcript: one fast Haiku call through
//! the `claude` CLI (subscription, no API key), and a deterministic transliteration if that call
//! fails, times out or returns something that lost words. Words are never dropped by this step.

use std::time::Duration;

/// How a transcript's script is treated after transcription.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Script {
    /// Devanagari becomes Roman Hinglish (default).
    Roman,
    /// Keep the Devanagari Codex wrote.
    Devanagari,
    /// Pass the provider text through untouched.
    AsIs,
}

impl Script {
    /// `AOS_VOICE_SCRIPT`: `roman` (default), `devanagari`, `as-is`.
    pub fn from_env() -> Script {
        Script::parse(&std::env::var("AOS_VOICE_SCRIPT").unwrap_or_default())
    }

    pub fn parse(s: &str) -> Script {
        match s.trim().to_ascii_lowercase().as_str() {
            "devanagari" | "hindi" => Script::Devanagari,
            "as-is" | "asis" | "as_is" | "raw" => Script::AsIs,
            _ => Script::Roman,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Script::Roman => "roman",
            Script::Devanagari => "devanagari",
            Script::AsIs => "as-is",
        }
    }
}

pub fn has_devanagari(s: &str) -> bool {
    s.chars().any(is_deva)
}

fn is_deva(c: char) -> bool {
    ('\u{0900}'..='\u{097F}').contains(&c)
}

/// Apply the setting. `claude` is the rewrite step (injected so tests need no CLI).
pub async fn apply<F, Fut>(text: String, script: Script, claude: F) -> String
where
    F: FnOnce(String) -> Fut,
    Fut: std::future::Future<Output = Option<String>>,
{
    if script != Script::Roman || !has_devanagari(&text) {
        return text;
    }
    match claude(text.clone()).await {
        Some(out) if acceptable(&text, &out) => out.trim().to_string(),
        _ => transliterate(&text),
    }
}

/// A rewrite that kept Devanagari, or lost half the text, is worse than the fallback.
fn acceptable(original: &str, out: &str) -> bool {
    let out = out.trim();
    !out.is_empty() && !has_devanagari(out) && out.chars().count() * 2 >= original.chars().count()
}

const PROMPT: &str = "You turn a Hindi or Hinglish speech transcript into Roman-script Hinglish, the way an Indian types on WhatsApp. \
Write English words in normal English spelling (code, add, PDF, task, gym). Keep every word and the order; \
do not translate, summarise, correct or add anything. Keep numbers and names. \
Reply with the converted transcript only, no quotes, no comment.";

/// One Haiku call through the `claude` CLI, killed after `limit`. None on any failure.
pub async fn haiku(text: String, limit: Duration) -> Option<String> {
    use std::process::Stdio;
    use tokio::io::AsyncWriteExt;
    let bin = std::env::var_os("AOS_CLAUDE_BIN").unwrap_or_else(|| "claude".into());
    let mut cmd = tokio::process::Command::new(bin);
    cmd.args([
        "-p",
        "--model",
        "haiku",
        "--tools=",
        "--strict-mcp-config",
        "--setting-sources",
        "project",
        "--no-session-persistence",
        "--output-format",
        "json",
    ])
    .env("MAX_THINKING_TOKENS", "0")
    .current_dir(std::env::temp_dir())
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .stderr(Stdio::null())
    .kill_on_drop(true);
    for (k, _) in std::env::vars() {
        if k.starts_with("CLAUDECODE") || k.starts_with("CLAUDE_CODE_") {
            cmd.env_remove(k);
        }
    }
    let mut child = cmd.spawn().ok()?;
    let mut stdin = child.stdin.take()?;
    let input = format!("{PROMPT}\n\nTranscript:\n{text}");
    let run = async move {
        stdin.write_all(input.as_bytes()).await.ok()?;
        drop(stdin);
        let out = child.wait_with_output().await.ok()?;
        let v: serde_json::Value = serde_json::from_slice(&out.stdout).ok()?;
        if v["is_error"].as_bool().unwrap_or(false) {
            return None;
        }
        v["result"].as_str().map(|s| s.trim().trim_matches('"').trim().to_string())
    };
    tokio::time::timeout(limit, run).await.ok().flatten()
}

/// The rewrite step used in production: Haiku with the 3 s budget.
pub async fn rewrite(text: String) -> Option<String> {
    let ms = std::env::var("AOS_VOICE_ROMAN_TIMEOUT_MS").ok().and_then(|v| v.parse().ok()).unwrap_or(3000);
    haiku(text, Duration::from_millis(ms)).await
}

fn consonant(c: char, nukta: bool) -> Option<&'static str> {
    Some(match (c, nukta) {
        ('क', true) => "q",
        ('ज', true) => "z",
        ('फ', true) => "f",
        ('ड', true) => "r",
        ('ढ', true) => "rh",
        ('क', _) => "k",
        ('ख', _) => "kh",
        ('ग', _) => "g",
        ('घ', _) => "gh",
        ('ङ', _) | ('ञ', _) | ('ण', _) | ('न', _) => "n",
        ('च', _) => "ch",
        ('छ', _) => "chh",
        ('ज', _) => "j",
        ('झ', _) => "jh",
        ('ट', _) | ('त', _) => "t",
        ('ठ', _) | ('थ', _) => "th",
        ('ड', _) | ('द', _) => "d",
        ('ढ', _) | ('ध', _) => "dh",
        ('प', _) => "p",
        ('फ', _) => "ph",
        ('ब', _) => "b",
        ('भ', _) => "bh",
        ('म', _) => "m",
        ('य', _) => "y",
        ('र', _) => "r",
        ('ल', _) | ('ळ', _) => "l",
        ('व', _) => "v",
        ('श', _) | ('ष', _) => "sh",
        ('स', _) => "s",
        ('ह', _) => "h",
        _ => return None,
    })
}

fn vowel(c: char) -> Option<&'static str> {
    Some(match c {
        'अ' => "a",
        'आ' => "aa",
        'इ' => "i",
        'ई' => "ee",
        'उ' => "u",
        'ऊ' => "oo",
        'ऋ' => "ri",
        'ए' => "e",
        'ऐ' => "ai",
        'ओ' | 'ऑ' => "o",
        'औ' => "au",
        _ => return None,
    })
}

fn matra(c: char) -> Option<&'static str> {
    Some(match c {
        'ा' => "aa",
        'ि' => "i",
        'ी' => "ee",
        'ु' => "u",
        'ू' => "oo",
        'ृ' => "ri",
        'े' | 'ॅ' => "e",
        'ै' => "ai",
        'ो' | 'ॉ' => "o",
        'ौ' => "au",
        _ => return None,
    })
}

/// One written unit of a word: its Roman letters, and whether a bare consonant still carries
/// the inherent "a" (which may be dropped).
struct Unit {
    text: String,
    schwa: bool,
    vowel: bool,
}

fn finish_word(units: &mut Vec<Unit>, out: &mut String) {
    let n = units.len();
    // Drop the inherent "a" at the end of a word, and inside it between two vowel-bearing units.
    // Left to right, so a dropped schwa is not counted as the next unit's vowel.
    for i in 0..n {
        if !units[i].schwa {
            continue;
        }
        let last = i + 1 == n;
        let inner = i > 0 && units[i - 1].vowel && i + 1 < n && units[i + 1].vowel;
        if last || inner {
            units[i].schwa = false;
            units[i].vowel = false;
        }
    }
    for u in units.drain(..) {
        out.push_str(&u.text);
        if u.schwa {
            out.push('a');
        }
    }
}

/// Deterministic Devanagari to Roman Hinglish. Other text is kept as written.
pub fn transliterate(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::new();
    let mut units: Vec<Unit> = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let nukta = chars.get(i + 1) == Some(&'\u{093C}');
        if let Some(r) = consonant(c, nukta) {
            units.push(Unit { text: r.into(), schwa: true, vowel: true });
            i += if nukta { 2 } else { 1 };
            continue;
        }
        if let Some(v) = vowel(c) {
            units.push(Unit { text: v.into(), schwa: false, vowel: true });
        } else if let Some(m) = matra(c) {
            // a long aa at the end of a word is written short: "jaana", "ka"
            let end = !chars.get(i + 1).is_some_and(|n| is_deva(*n) && *n != '।');
            let m = if m == "aa" && end { "a" } else { m };
            if let Some(u) = units.last_mut() {
                u.schwa = false;
                u.text.push_str(m);
            } else {
                out.push_str(m);
            }
        } else if c == '\u{094D}' {
            if let Some(u) = units.last_mut() {
                u.schwa = false;
                u.vowel = false;
            }
        } else if c == 'ं' || c == 'ँ' {
            if let Some(u) = units.last_mut() {
                u.text.push('n');
            } else {
                out.push('n');
            }
        } else if c == 'ः' {
            if let Some(u) = units.last_mut() {
                u.text.push('h');
            }
        } else if c == '\u{093C}' {
            // stray nukta
        } else if c == '।' || c == '॥' {
            finish_word(&mut units, &mut out);
            out.push('.');
        } else if ('०'..='९').contains(&c) {
            finish_word(&mut units, &mut out);
            out.push(char::from(b'0' + (c as u32 - '०' as u32) as u8));
        } else {
            finish_word(&mut units, &mut out);
            out.push(c);
        }
        i += 1;
    }
    finish_word(&mut units, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    static CALLED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

    #[test]
    fn setting_parses_with_roman_default() {
        assert_eq!(Script::parse(""), Script::Roman);
        assert_eq!(Script::parse("nonsense"), Script::Roman);
        assert_eq!(Script::parse("Devanagari"), Script::Devanagari);
        assert_eq!(Script::parse("as-is"), Script::AsIs);
    }

    #[test]
    fn transliterates_common_words() {
        assert_eq!(transliterate("कल"), "kal");
        assert_eq!(transliterate("काम"), "kaam");
        assert_eq!(transliterate("है"), "hai");
        assert_eq!(transliterate("फिर"), "phir");
        assert_eq!(transliterate("जाना"), "jaana");
        assert_eq!(transliterate("समझना"), "samjhana");
        assert_eq!(transliterate("का"), "ka");
    }

    #[test]
    fn transliteration_keeps_latin_digits_and_punctuation() {
        let out = transliterate("कल gym जाना है। 2 बजे, ठीक है?");
        assert!(out.starts_with("kal gym jaana hai."), "{out}");
        assert!(out.contains("2 baje, theek hai?"), "{out}");
        assert!(!has_devanagari(&out));
    }

    #[tokio::test]
    async fn roman_uses_the_rewrite_when_it_is_good() {
        let got = apply("कल जिम जाना है".into(), Script::Roman, |_| async { Some("Kal gym jaana hai".to_string()) }).await;
        assert_eq!(got, "Kal gym jaana hai");
    }

    #[tokio::test]
    async fn roman_falls_back_when_the_rewrite_fails_or_is_bad() {
        let src = "कल जाना है";
        for bad in [None, Some(String::new()), Some("कल जाना है".to_string()), Some("k".to_string())] {
            let got = apply(src.into(), Script::Roman, |_| async move { bad }).await;
            assert_eq!(got, "kal jaana hai");
        }
    }

    #[tokio::test]
    async fn other_settings_and_latin_text_are_untouched() {
        let hindi = "कल जाना है".to_string();
        let d = apply(hindi.clone(), Script::Devanagari, |_| async { Some("x".to_string()) }).await;
        assert_eq!(d, hindi);
        let a = apply(hindi.clone(), Script::AsIs, |_| async { Some("x".to_string()) }).await;
        assert_eq!(a, hindi);
        let latin = apply("kal gym jaana hai".into(), Script::Roman, |_| async {
            CALLED.store(true, std::sync::atomic::Ordering::SeqCst);
            None
        })
        .await;
        assert!(!CALLED.load(std::sync::atomic::Ordering::SeqCst));
        assert_eq!(latin, "kal gym jaana hai");
    }
}
