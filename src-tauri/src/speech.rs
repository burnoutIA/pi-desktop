//! Extrait, au fil du streaming, le texte à vocaliser (entre `<oral>` et `</oral>`).

const OPEN: &str = "<oral>";
const CLOSE: &str = "</oral>";

#[derive(Debug, PartialEq)]
pub enum Speech {
    Word(String),
    End,
}

#[derive(Default)]
pub struct OralExtractor {
    buf: String,
    scan: usize,
    inside: bool,
    done: bool,
    word: String,
}

fn floor_boundary(s: &str, mut i: usize) -> usize {
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

impl OralExtractor {
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    pub fn push(&mut self, delta: &str) -> Vec<Speech> {
        let mut out = Vec::new();
        if self.done {
            return out;
        }
        self.buf.push_str(delta);
        loop {
            if !self.inside {
                if let Some(p) = self.buf[self.scan..].find(OPEN) {
                    self.inside = true;
                    self.scan += p + OPEN.len();
                    continue;
                }
                // Garde de quoi reconnaître une balise coupée entre deux deltas.
                self.scan = floor_boundary(&self.buf, self.buf.len().saturating_sub(OPEN.len()));
                return out;
            }
            if let Some(p) = self.buf[self.scan..].find(CLOSE) {
                let end = self.scan + p;
                let text = self.buf[self.scan..end].to_string();
                self.feed(&text, &mut out);
                self.flush(&mut out);
                out.push(Speech::End);
                self.inside = false;
                self.done = true;
                return out;
            }
            let safe = floor_boundary(&self.buf, self.buf.len().saturating_sub(CLOSE.len()));
            if safe > self.scan {
                let text = self.buf[self.scan..safe].to_string();
                self.feed(&text, &mut out);
                self.scan = safe;
            }
            return out;
        }
    }

    /// Fin du message : si la balise fermante manque, on termine quand même.
    pub fn finish(&mut self) -> Vec<Speech> {
        let mut out = Vec::new();
        if self.inside && !self.done {
            let text = self.buf[self.scan..].to_string();
            self.feed(&text, &mut out);
            self.flush(&mut out);
            out.push(Speech::End);
            self.done = true;
            self.inside = false;
        }
        out
    }

    pub fn started(&self) -> bool {
        self.inside || self.done
    }

    fn feed(&mut self, text: &str, out: &mut Vec<Speech>) {
        for c in text.chars() {
            if c.is_whitespace() {
                self.flush(out);
            } else {
                self.word.push(c);
            }
        }
    }

    fn flush(&mut self, out: &mut Vec<Speech>) {
        // Une balise mal écrite (`</Oral>`, `</oral >`, `<oral>` en double…) ne se lit pas.
        let w = clean_word(&strip_oral_tags(&self.word));
        self.word.clear();
        if !w.is_empty() {
            out.push(Speech::Word(w));
        }
    }
}

/// Retire d'un mot les balises `oral` mal écrites (`</Oral>`, `<oral`, `oral>`…), même collées
/// au texte (`fini.</Oral>`). Le mot « oral » sans chevron (un examen oral) est gardé.
fn strip_oral_tags(w: &str) -> String {
    if !w.contains(['<', '>']) {
        return w.to_string();
    }
    let mut out = w.to_string();
    // Minuscules ASCII seulement : les positions restent valables dans le mot d'origine.
    for pat in ["</oral>", "<oral>", "</oral", "<oral", "oral>"] {
        while let Some(p) = out.to_ascii_lowercase().find(pat) {
            out.replace_range(p..p + pat.len(), "");
        }
    }
    out
}

/// Retire ce qui ne se prononce pas.
pub fn clean_word(w: &str) -> String {
    w.chars()
        .filter(|c| !matches!(c, '*' | '_' | '`' | '#' | '<' | '>' | '[' | ']' | '|'))
        .map(|c| match c {
            '“' | '”' | '«' | '»' => '"',
            '‘' | '’' => '\'',
            c => c,
        })
        .collect::<String>()
        .trim_matches('"')
        .to_string()
}

/// Repli si pi n'a pas produit de bloc `<oral>` : on lit le début du texte, sans code.
pub fn fallback_speech(text: &str) -> String {
    let mut out = String::new();
    let mut in_code = false;
    for line in text.lines() {
        let line: String = line.split_whitespace().map(strip_oral_tags).collect::<Vec<_>>().join(" ");
        let t = line.trim();
        if t.starts_with("```") {
            in_code = !in_code;
            continue;
        }
        if in_code || t.is_empty() || t.starts_with('|') {
            continue;
        }
        let t = t.trim_start_matches(|c: char| c == '#' || c == '-' || c == '>' || c == '*').trim();
        out.push_str(t);
        out.push(' ');
        if out.len() > 280 {
            break;
        }
    }
    // Coupe à la fin de la dernière phrase complète si possible.
    if out.len() > 280 {
        let cut = floor_boundary(&out, 280);
        let head = &out[..cut];
        if let Some(p) = head.rfind(['.', '!', '?']) {
            return head[..=p].to_string();
        }
        return head.to_string();
    }
    out.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(v: &[Speech]) -> Vec<String> {
        v.iter()
            .filter_map(|s| match s {
                Speech::Word(w) => Some(w.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn malformed_tags_are_not_spoken() {
        let mut e = OralExtractor::default();
        let mut all = e.push("Texte.\n<oral>J'ai fini l'examen oral.</Oral>");
        all.extend(e.finish());
        assert_eq!(words(&all), ["J'ai", "fini", "l'examen", "oral."]);

        let mut e = OralExtractor::default();
        let mut all = e.push("<oral>C'est prêt. <oral>");
        all.extend(e.finish());
        assert_eq!(words(&all), ["C'est", "prêt."]);

        let mut e = OralExtractor::default();
        let mut all = e.push("<oral>Voilà. </oral >");
        all.extend(e.finish());
        assert_eq!(words(&all), ["Voilà."]);
    }

    #[test]
    fn streams_split_tags() {
        let mut e = OralExtractor::default();
        let mut all = Vec::new();
        for d in ["Fini.\n<or", "al>J'ai cré", "é le **fichier** ", "main.rs.</o", "ral> après"] {
            all.extend(e.push(d));
        }
        assert_eq!(words(&all), ["J'ai", "créé", "le", "fichier", "main.rs."]);
        assert_eq!(all.last(), Some(&Speech::End));
    }

    #[test]
    fn finish_without_close() {
        let mut e = OralExtractor::default();
        let mut all = e.push("<oral>Bonjour à tous");
        all.extend(e.finish());
        assert_eq!(words(&all), ["Bonjour", "à", "tous"]);
    }

    #[test]
    fn fallback_skips_code() {
        let t = "# Titre\nJ'ai corrigé le bug.\n```rust\nfn x(){}\n```\nC'est bon.";
        assert_eq!(fallback_speech(t), "Titre J'ai corrigé le bug. C'est bon.");
    }
}
