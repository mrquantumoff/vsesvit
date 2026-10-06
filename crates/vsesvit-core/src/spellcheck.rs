//! Spell checking's languages. A language is named as its Hunspell dictionary is (`en_US`,
//! `uk_UA`, `de`). Until the user chooses languages ([`crate::prefs::keys::SPELLCHECK_LANGUAGES`]),
//! spell checking uses the installed dictionaries that match the system's languages, as Chrome
//! starts from its UI language. A choice synced from another device keeps the languages this one
//! has no dictionary for, and checks the rest.

use std::path::PathBuf;

/// Chrome's English name for each language, and the region its dictionary most likely has when
/// a locale names only the language (`en` is `en_US`, `pt` is `pt_BR`).
const LANGUAGES: &[(&str, &str, &str)] = &[
    ("af", "Afrikaans", "ZA"),
    ("an", "Aragonese", "ES"),
    ("ar", "Arabic", "EG"),
    ("as", "Assamese", "IN"),
    ("ast", "Asturian", "ES"),
    ("az", "Azerbaijani", "AZ"),
    ("be", "Belarusian", "BY"),
    ("bg", "Bulgarian", "BG"),
    ("bn", "Bangla", "BD"),
    ("bo", "Tibetan", "CN"),
    ("br", "Breton", "FR"),
    ("bs", "Bosnian", "BA"),
    ("ca", "Catalan", "ES"),
    ("cs", "Czech", "CZ"),
    ("cy", "Welsh", "GB"),
    ("da", "Danish", "DK"),
    ("de", "German", "DE"),
    ("dz", "Dzongkha", "BT"),
    ("el", "Greek", "GR"),
    ("en", "English", "US"),
    ("eo", "Esperanto", ""),
    ("es", "Spanish", "ES"),
    ("et", "Estonian", "EE"),
    ("eu", "Basque", "ES"),
    ("fa", "Persian", "IR"),
    ("fi", "Finnish", "FI"),
    ("fo", "Faroese", "FO"),
    ("fr", "French", "FR"),
    ("fy", "Western Frisian", "NL"),
    ("ga", "Irish", "IE"),
    ("gd", "Scottish Gaelic", "GB"),
    ("gl", "Galician", "ES"),
    ("gu", "Gujarati", "IN"),
    ("gv", "Manx", "IM"),
    ("he", "Hebrew", "IL"),
    ("hi", "Hindi", "IN"),
    ("hr", "Croatian", "HR"),
    ("hu", "Hungarian", "HU"),
    ("hy", "Armenian", "AM"),
    ("id", "Indonesian", "ID"),
    ("is", "Icelandic", "IS"),
    ("it", "Italian", "IT"),
    ("ka", "Georgian", "GE"),
    ("kk", "Kazakh", "KZ"),
    ("km", "Khmer", "KH"),
    ("kmr", "Northern Kurdish", "TR"),
    ("kn", "Kannada", "IN"),
    ("ko", "Korean", "KR"),
    ("ku", "Kurdish", "TR"),
    ("lb", "Luxembourgish", "LU"),
    ("lo", "Lao", "LA"),
    ("lt", "Lithuanian", "LT"),
    ("lv", "Latvian", "LV"),
    ("mk", "Macedonian", "MK"),
    ("ml", "Malayalam", "IN"),
    ("mn", "Mongolian", "MN"),
    ("mr", "Marathi", "IN"),
    ("ms", "Malay", "MY"),
    ("nb", "Norwegian Bokmål", "NO"),
    ("ne", "Nepali", "NP"),
    ("nl", "Dutch", "NL"),
    ("nn", "Norwegian Nynorsk", "NO"),
    ("no", "Norwegian", "NO"),
    ("nr", "South Ndebele", "ZA"),
    ("nso", "Northern Sotho", "ZA"),
    ("oc", "Occitan", "FR"),
    ("or", "Odia", "IN"),
    ("pa", "Punjabi", "IN"),
    ("pl", "Polish", "PL"),
    ("pt", "Portuguese", "BR"),
    ("ro", "Romanian", "RO"),
    ("ru", "Russian", "RU"),
    ("rw", "Kinyarwanda", "RW"),
    ("si", "Sinhala", "LK"),
    ("sk", "Slovak", "SK"),
    ("sl", "Slovenian", "SI"),
    ("sq", "Albanian", "AL"),
    ("sr", "Serbian", "RS"),
    ("ss", "Swati", "ZA"),
    ("st", "Southern Sotho", "ZA"),
    ("sv", "Swedish", "SE"),
    ("sw", "Swahili", "TZ"),
    ("ta", "Tamil", "IN"),
    ("te", "Telugu", "IN"),
    ("tg", "Tajik", "TJ"),
    ("th", "Thai", "TH"),
    ("tl", "Tagalog", "PH"),
    ("tn", "Tswana", "ZA"),
    ("tr", "Turkish", "TR"),
    ("ts", "Tsonga", "ZA"),
    ("uk", "Ukrainian", "UA"),
    ("uz", "Uzbek", "UZ"),
    ("ve", "Venda", "ZA"),
    ("vi", "Vietnamese", "VN"),
    ("xh", "Xhosa", "ZA"),
    ("zu", "Zulu", "ZA"),
];

/// Chrome's English name for each region a dictionary is commonly made for.
const REGIONS: &[(&str, &str)] = &[
    ("AL", "Albania"),
    ("AM", "Armenia"),
    ("AR", "Argentina"),
    ("AT", "Austria"),
    ("AU", "Australia"),
    ("AZ", "Azerbaijan"),
    ("BA", "Bosnia & Herzegovina"),
    ("BD", "Bangladesh"),
    ("BE", "Belgium"),
    ("BG", "Bulgaria"),
    ("BO", "Bolivia"),
    ("BR", "Brazil"),
    ("BT", "Bhutan"),
    ("BY", "Belarus"),
    ("BZ", "Belize"),
    ("CA", "Canada"),
    ("CH", "Switzerland"),
    ("CL", "Chile"),
    ("CN", "China"),
    ("CO", "Colombia"),
    ("CR", "Costa Rica"),
    ("CU", "Cuba"),
    ("CZ", "Czechia"),
    ("DE", "Germany"),
    ("DK", "Denmark"),
    ("DO", "Dominican Republic"),
    ("EC", "Ecuador"),
    ("EE", "Estonia"),
    ("EG", "Egypt"),
    ("ES", "Spain"),
    ("FI", "Finland"),
    ("FO", "Faroe Islands"),
    ("FR", "France"),
    ("GB", "United Kingdom"),
    ("GE", "Georgia"),
    ("GH", "Ghana"),
    ("GR", "Greece"),
    ("GT", "Guatemala"),
    ("HN", "Honduras"),
    ("HR", "Croatia"),
    ("HU", "Hungary"),
    ("ID", "Indonesia"),
    ("IE", "Ireland"),
    ("IL", "Israel"),
    ("IM", "Isle of Man"),
    ("IN", "India"),
    ("IR", "Iran"),
    ("IS", "Iceland"),
    ("IT", "Italy"),
    ("JM", "Jamaica"),
    ("KH", "Cambodia"),
    ("KR", "South Korea"),
    ("KZ", "Kazakhstan"),
    ("LA", "Laos"),
    ("LI", "Liechtenstein"),
    ("LK", "Sri Lanka"),
    ("LT", "Lithuania"),
    ("LU", "Luxembourg"),
    ("LV", "Latvia"),
    ("MC", "Monaco"),
    ("ME", "Montenegro"),
    ("MK", "North Macedonia"),
    ("MN", "Mongolia"),
    ("MX", "Mexico"),
    ("MY", "Malaysia"),
    ("NA", "Namibia"),
    ("NG", "Nigeria"),
    ("NI", "Nicaragua"),
    ("NL", "Netherlands"),
    ("NO", "Norway"),
    ("NP", "Nepal"),
    ("NZ", "New Zealand"),
    ("PA", "Panama"),
    ("PE", "Peru"),
    ("PH", "Philippines"),
    ("PK", "Pakistan"),
    ("PL", "Poland"),
    ("PR", "Puerto Rico"),
    ("PT", "Portugal"),
    ("PY", "Paraguay"),
    ("RO", "Romania"),
    ("RS", "Serbia"),
    ("RU", "Russia"),
    ("RW", "Rwanda"),
    ("SE", "Sweden"),
    ("SG", "Singapore"),
    ("SI", "Slovenia"),
    ("SK", "Slovakia"),
    ("SV", "El Salvador"),
    ("TH", "Thailand"),
    ("TJ", "Tajikistan"),
    ("TR", "Türkiye"),
    ("TT", "Trinidad & Tobago"),
    ("TZ", "Tanzania"),
    ("UA", "Ukraine"),
    ("US", "United States"),
    ("UY", "Uruguay"),
    ("UZ", "Uzbekistan"),
    ("VE", "Venezuela"),
    ("VN", "Vietnam"),
    ("ZA", "South Africa"),
    ("ZW", "Zimbabwe"),
];

/// The dictionaries installed on this device, and which of them the system's languages ask for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Dictionaries {
    /// Sorted, each once.
    installed: Vec<String>,
    /// The installed dictionaries for the system's languages, the most preferred first.
    system: Vec<String>,
}

impl Dictionaries {
    /// `locales` as the system lists them ([`system_locales`]), the most preferred first.
    pub fn new(mut installed: Vec<String>, locales: &[String]) -> Dictionaries {
        installed.sort();
        installed.dedup();
        let mut system: Vec<String> = Vec::new();
        for found in locales.iter().filter_map(|locale| dictionary_for(locale, &installed)) {
            if !system.iter().any(|language| language == found) {
                system.push(found.to_owned());
            }
        }
        Dictionaries { installed, system }
    }

    pub fn installed(&self) -> &[String] {
        &self.installed
    }

    /// The languages to check: those of the user's choice that are installed here, in its order,
    /// or the system's when the user has chosen none.
    pub fn checked(&self, chosen: Option<&[String]>) -> Vec<String> {
        let Some(chosen) = chosen else { return self.system.clone() };
        let mut checked: Vec<String> = Vec::new();
        for language in chosen {
            if self.installed.contains(language) && !checked.contains(language) {
                checked.push(language.clone());
            }
        }
        checked
    }

    /// The choice to store once `language` is turned on or off. The first choice starts from the
    /// system's languages; languages chosen on another device stay, installed here or not.
    pub fn choose(&self, chosen: Option<Vec<String>>, language: &str, on: bool) -> Vec<String> {
        let mut chosen = chosen.unwrap_or_else(|| self.system.clone());
        chosen.retain(|l| l != language);
        if on {
            chosen.push(language.to_owned());
        }
        chosen
    }
}

/// The dictionaries in `dirs`: each `<language>.dic` that has its `<language>.aff`, as Hunspell
/// needs both. A directory that cannot be read has none.
pub fn installed_in(dirs: &[PathBuf]) -> Vec<String> {
    let mut found: Vec<String> = dirs
        .iter()
        .filter_map(|dir| std::fs::read_dir(dir).ok())
        .flatten()
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            if path.extension()? != "dic" || !path.with_extension("aff").is_file() {
                return None;
            }
            path.file_stem()?.to_str().map(str::to_owned)
        })
        .collect();
    found.sort();
    found.dedup();
    found
}

/// The system's languages, the most preferred first: `LANGUAGE`'s list, then the locale of
/// `LC_ALL` / `LC_MESSAGES` / `LANG`.
pub fn system_locales() -> Vec<String> {
    locales_from(std::env::var("LANGUAGE").ok(), crate::extensions::ui_locale())
}

fn locales_from(language: Option<String>, ui_locale: String) -> Vec<String> {
    let mut locales: Vec<String> = language.iter().flat_map(|list| list.split(':')).filter(|l| !l.is_empty()).map(str::to_owned).collect();
    locales.push(ui_locale);
    locales
}

/// The installed dictionary for `locale` (`uk_UA.UTF-8`, `en-GB`, `C`): its own, else its
/// language's without a region, else the one for the region the language most likely means,
/// else the first for the language.
fn dictionary_for<'a>(locale: &str, installed: &'a [String]) -> Option<&'a str> {
    let base = locale.split(['.', '@']).next().unwrap_or_default().replace('-', "_");
    let base = if base == "C" || base == "POSIX" { "en".to_owned() } else { base };
    let language = base.split('_').next().unwrap_or_default();
    if language.is_empty() {
        return None;
    }
    let region = LANGUAGES.iter().find(|(code, ..)| *code == language).map_or("", |(.., region)| *region);
    let prefix = format!("{language}_");
    [base.clone(), language.to_owned(), format!("{prefix}{region}")]
        .iter()
        .find_map(|tag| installed.iter().find(|installed| *installed == tag))
        .or_else(|| installed.iter().find(|installed| installed.starts_with(&prefix)))
        .map(String::as_str)
}

/// How Settings names `language`: "English (United States)" for `en_US`, "Ukrainian" for `uk`.
/// A dictionary named otherwise (`de_DE_frami`) shows as named.
pub fn display_name(language: &str) -> String {
    let name = |code: &str| LANGUAGES.iter().find(|(c, ..)| *c == code).map(|(_, name, _)| *name);
    let region = |code: &str| REGIONS.iter().find(|(c, _)| *c == code).map(|(_, name)| *name);
    let known = match language.split_once('_') {
        None => name(language).map(str::to_owned),
        Some((lang, reg)) => name(lang).zip(region(reg)).map(|(lang, reg)| format!("{lang} ({reg})")),
    };
    known.unwrap_or_else(|| language.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tags(tags: &[&str]) -> Vec<String> {
        tags.iter().map(|t| (*t).to_owned()).collect()
    }

    #[test]
    fn a_dictionary_needs_its_affix_file_and_counts_once() {
        let root = std::env::temp_dir().join(format!("vsesvit-dictionaries-{}", uuid::Uuid::new_v4().simple()));
        let (hunspell, myspell) = (root.join("hunspell"), root.join("myspell"));
        for (dir, files) in [
            (&hunspell, &["en_US.dic", "en_US.aff", "uk_UA.dic", "uk_UA.aff", "de_DE.dic", "README"][..]),
            (&myspell, &["en_US.dic", "en_US.aff", "fr.aff"][..]),
        ] {
            std::fs::create_dir_all(dir).unwrap();
            for file in files {
                std::fs::write(dir.join(file), "").unwrap();
            }
        }
        let found = installed_in(&[hunspell, myspell, root.join("missing")]);
        std::fs::remove_dir_all(&root).unwrap();
        assert_eq!(found, tags(&["en_US", "uk_UA"]));
    }

    #[test]
    fn the_system_languages_pick_the_closest_installed_dictionary() {
        let installed = tags(&["de_DE", "en_AU", "en_GB", "en_US", "fr", "pt_BR", "pt_PT", "uk_UA"]);
        for (locale, expected) in [
            ("uk_UA.UTF-8", Some("uk_UA")),
            ("en-GB", Some("en_GB")),
            ("C", Some("en_US")),
            ("C.UTF-8", Some("en_US")),
            ("en_IE", Some("en_US")),
            ("de_AT@euro", Some("de_DE")),
            ("fr_CA", Some("fr")),
            ("pt", Some("pt_BR")),
            ("ja_JP", None),
            ("", None),
        ] {
            assert_eq!(dictionary_for(locale, &installed), expected, "{locale:?}");
        }
        let one_english = tags(&["en_GB", "en_ZA"]);
        assert_eq!(dictionary_for("en", &one_english), Some("en_GB"), "any dictionary of the language is better than none");
    }

    #[test]
    fn language_lists_the_locales_before_the_ui_locale() {
        assert_eq!(locales_from(Some("uk:en_GB:".into()), "uk_UA".into()), tags(&["uk", "en_GB", "uk_UA"]));
        assert_eq!(locales_from(None, "en".into()), tags(&["en"]));
    }

    #[test]
    fn the_system_languages_check_until_the_user_chooses() {
        let dictionaries = Dictionaries::new(tags(&["uk_UA", "en_US", "de_DE", "en_US"]), &tags(&["uk", "en_US", "uk_UA", "ja"]));
        assert_eq!(dictionaries.installed(), tags(&["de_DE", "en_US", "uk_UA"]));
        assert_eq!(dictionaries.checked(None), tags(&["uk_UA", "en_US"]));

        let chosen = dictionaries.choose(None, "de_DE", true);
        assert_eq!(chosen, tags(&["uk_UA", "en_US", "de_DE"]), "the first choice starts from the system's");
        let chosen = dictionaries.choose(Some(chosen), "uk_UA", false);
        assert_eq!(dictionaries.checked(Some(&chosen)), tags(&["en_US", "de_DE"]));

        let synced = tags(&["fr_FR", "de_DE"]);
        assert_eq!(dictionaries.checked(Some(&synced)), tags(&["de_DE"]), "another device's language not installed here");
        assert_eq!(dictionaries.choose(Some(synced), "en_US", true), tags(&["fr_FR", "de_DE", "en_US"]), "and it stays chosen");
        assert_eq!(dictionaries.checked(Some(&[])), Vec::<String>::new(), "the user can turn every language off");
    }

    #[test]
    fn languages_are_named_as_chrome_names_them() {
        assert_eq!(display_name("en_US"), "English (United States)");
        assert_eq!(display_name("uk"), "Ukrainian");
        assert_eq!(display_name("nb_NO"), "Norwegian Bokmål (Norway)");
        assert_eq!(display_name("de_DE_frami"), "de_DE_frami");
        assert_eq!(display_name("xx_YY"), "xx_YY");
    }

    #[test]
    fn every_likely_region_has_a_name() {
        for (code, _, region) in LANGUAGES {
            assert!(region.is_empty() || REGIONS.iter().any(|(r, _)| r == region), "{code}: {region}");
        }
        assert!(LANGUAGES.is_sorted_by_key(|(code, ..)| *code) && REGIONS.is_sorted_by_key(|(code, _)| *code));
    }
}
