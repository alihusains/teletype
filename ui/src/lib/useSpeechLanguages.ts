import { invoke } from "@tauri-apps/api/core";
import { useEffect, useState } from "react";

export interface SpeechLanguage {
  code: string;
  name: string;
}

/**
 * Fetches the full list of Whisper-supported languages (99) from the backend.
 * Falls back to a hardcoded list if the `list_speech_languages` Tauri command
 * is not yet available (Phase A before Phase B lands).
 */
export function useSpeechLanguages(): SpeechLanguage[] {
  const [languages, setLanguages] = useState<SpeechLanguage[]>(FALLBACK_LANGUAGES);

  useEffect(() => {
    let cancelled = false;
    invoke<SpeechLanguage[]>("list_speech_languages")
      .then((langs) => {
        if (!cancelled && langs.length > 0) setLanguages(langs);
      })
      .catch(() => {
        // Command not available yet; keep the fallback list.
      });
    return () => {
      cancelled = true;
    };
  }, []);

  return languages;
}

/// Fallback: the 99 languages supported by Whisper (from whisper.cpp's
/// language table). Used when the `list_speech_languages` backend command
/// is not yet available.
const FALLBACK_LANGUAGES: SpeechLanguage[] = [
  { code: "en", name: "English" },
  { code: "zh", name: "Chinese" },
  { code: "de", name: "German" },
  { code: "es", name: "Spanish" },
  { code: "ru", name: "Russian" },
  { code: "ko", name: "Korean" },
  { code: "fr", name: "French" },
  { code: "ja", name: "Japanese" },
  { code: "pt", name: "Portuguese" },
  { code: "fi", name: "Finnish" },
  { code: "pl", name: "Polish" },
  { code: "ca", name: "Catalan" },
  { code: "nl", name: "Dutch" },
  { code: "tr", name: "Turkish" },
  { code: "ar", name: "Arabic" },
  { code: "sv", name: "Swedish" },
  { code: "it", name: "Italian" },
  { code: "hi", name: "Hindi" },
  { code: "da", name: "Danish" },
  { code: "he", name: "Hebrew" },
  { code: "fa", name: "Persian" },
  { code: "no", name: "Norwegian" },
  { code: "th", name: "Thai" },
  { code: "ur", name: "Urdu" },
  { code: "hr", name: "Croatian" },
  { code: "bg", name: "Bulgarian" },
  { code: "el", name: "Greek" },
  { code: "ro", name: "Romanian" },
  { code: "hu", name: "Hungarian" },
  { code: "lt", name: "Lithuanian" },
  { code: "la", name: "Latin" },
  { code: "mi", name: "Maori" },
  { code: "ml", name: "Malayalam" },
  { code: "cy", name: "Welsh" },
  { code: "sk", name: "Slovak" },
  { code: "te", name: "Telugu" },
  { code: "lv", name: "Latvian" },
  { code: "bn", name: "Bengali" },
  { code: "sr", name: "Serbian" },
  { code: "az", name: "Azerbaijani" },
  { code: "sl", name: "Slovenian" },
  { code: "kn", name: "Kannada" },
  { code: "et", name: "Estonian" },
  { code: "mk", name: "Macedonian" },
  { code: "br", name: "Breton" },
  { code: "uk", name: "Ukrainian" },
  { code: "hy", name: "Armenian" },
  { code: "mn", name: "Mongolian" },
  { code: "bs", name: "Bosnian" },
  { code: "kk", name: "Kazakh" },
  { code: "sq", name: "Albanian" },
  { code: "sw", name: "Swahili" },
  { code: "gl", name: "Galician" },
  { code: "mr", name: "Marathi" },
  { code: "pa", name: "Punjabi" },
  { code: "si", name: "Sinhala" },
  { code: "id", name: "Indonesian" },
  { code: "vi", name: "Vietnamese" },
  { code: "tl", name: "Tagalog" },
  { code: "my", name: "Burmese" },
  { code: "ne", name: "Nepali" },
  { code: "ta", name: "Tamil" },
  { code: "oc", name: "Occitan" },
  { code: "gu", name: "Gujarati" },
  { code: "be", name: "Belarusian" },
  { code: "is", name: "Icelandic" },
  { code: "af", name: "Afrikaans" },
  { code: "jw", name: "Javanese" },
  { code: "am", name: "Amharic" },
  { code: "lo", name: "Lao" },
  { code: "uz", name: "Uzbek" },
  { code: "su", name: "Sundanese" },
  { code: "ka", name: "Georgian" },
  { code: "mg", name: "Malagasy" },
  { code: "yue", name: "Cantonese" },
  { code: "ha", name: "Hausa" },
  { code: "yo", name: "Yoruba" },
  { code: "xh", name: "Xhosa" },
  { code: "lb", name: "Luxembourgish" },
  { code: "ht", name: "Haitian Creole" },
  { code: "ps", name: "Pashto" },
  { code: "mt", name: "Maltese" },
  { code: "co", name: "Corsican" },
  { code: "tg", name: "Tajik" },
  { code: "ny", name: "Nyanja" },
  { code: "sd", name: "Sindhi" },
  { code: "gd", name: "Scottish Gaelic" },
  { code: "lg", name: "Luganda" },
  { code: "or", name: "Odia" },
  { code: "ceb", name: "Cebuano" },
  { code: "haw", name: "Hawaiian" },
  { code: "ln", name: "Lingala" },
  { code: "rw", name: "Kinyarwanda" },
  { code: "so", name: "Somali" },
  { code: "zu", name: "Zulu" },
];
