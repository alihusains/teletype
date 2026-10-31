import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Icon } from "../Icon";
import { BAR_COUNT, barsForWidth, clockLabel, resamplePeaks, waveformFromWav, type Waveform } from "./waveform";

/** Decoded recordings kept so re-expanding a card does not refetch. */
const CACHE_LIMIT = 10;
const cache = new Map<string, { bytes: ArrayBuffer; wave: Waveform }>();

function cacheGet(id: string) {
  const hit = cache.get(id);
  if (hit) {
    // Refresh recency: a Map iterates in insertion order.
    cache.delete(id);
    cache.set(id, hit);
  }
  return hit;
}

function cachePut(id: string, value: { bytes: ArrayBuffer; wave: Waveform }) {
  cache.set(id, value);
  while (cache.size > CACHE_LIMIT) {
    const oldest = cache.keys().next().value;
    if (oldest === undefined) break;
    cache.delete(oldest);
  }
}

/** Test seam: forget every cached recording. */
export function clearAudioCache() {
  cache.clear();
}

/** The one audio element allowed to play; starting another pauses it. */
let playing: HTMLAudioElement | null = null;

/** Bars quieter than this draw as a dot, as silence does in the reference. */
const DOT_THRESHOLD = 0.08;

type Load =
  | { status: "loading" }
  | { status: "error" }
  | { status: "ready"; wave: Waveform; url: string };

/**
 * The expanded card's recording: a play button, a seekable waveform and the
 * clock. The audio is fetched by entry id when the panel mounts (the card
 * mounts it on expand), never on list load.
 */
export function AudioPlayer({ id }: { id: string }) {
  const [load, setLoad] = useState<Load>({ status: "loading" });
  const [isPlaying, setIsPlaying] = useState(false);
  const [time, setTime] = useState(0);
  const [bars, setBars] = useState(BAR_COUNT);
  const audioRef = useRef<HTMLAudioElement | null>(null);
  const barsRef = useRef<HTMLDivElement | null>(null);

  // Draw only as many bars as the row can hold at their narrowest, so the
  // waveform never runs into the clock at the minimum window width.
  useEffect(() => {
    const el = barsRef.current;
    if (!el || typeof ResizeObserver === "undefined") return;
    const measure = () => setBars(barsForWidth(el.clientWidth));
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(el);
    return () => observer.disconnect();
  }, [load.status]);

  useEffect(() => {
    let alive = true;
    let url: string | null = null;
    const ready = (entry: { bytes: ArrayBuffer; wave: Waveform }) => {
      url = URL.createObjectURL(new Blob([entry.bytes], { type: "audio/wav" }));
      setLoad({ status: "ready", wave: entry.wave, url });
    };
    const hit = cacheGet(id);
    if (hit) {
      ready(hit);
    } else {
      invoke<ArrayBuffer>("get_dictation_audio", { id })
        .then((raw) => {
          if (!alive) return;
          const bytes = raw instanceof ArrayBuffer ? raw : new Uint8Array(raw as ArrayLike<number>).buffer;
          const wave = waveformFromWav(bytes);
          if (!wave) {
            setLoad({ status: "error" });
            return;
          }
          const entry = { bytes, wave };
          cachePut(id, entry);
          ready(entry);
        })
        .catch(() => {
          if (alive) setLoad({ status: "error" });
        });
    }
    return () => {
      alive = false;
      const audio = audioRef.current;
      if (audio) {
        audio.pause();
        if (playing === audio) playing = null;
      }
      if (url) URL.revokeObjectURL(url);
    };
  }, [id]);

  const duration = load.status === "ready" ? load.wave.durationSec : 0;

  // Smooth progress while playing; `timeupdate` alone ticks ~4 times a second.
  useEffect(() => {
    if (!isPlaying) return;
    let raf = 0;
    const tick = () => {
      const a = audioRef.current;
      if (a) setTime(a.currentTime);
      raf = requestAnimationFrame(tick);
    };
    raf = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(raf);
  }, [isPlaying]);

  const toggle = useCallback(async () => {
    const audio = audioRef.current;
    if (!audio || load.status !== "ready") return;
    if (!audio.paused) {
      audio.pause();
      return;
    }
    if (playing && playing !== audio) playing.pause();
    playing = audio;
    try {
      await audio.play();
    } catch {
      setIsPlaying(false);
    }
  }, [load.status]);

  const seek = useCallback(
    (sec: number) => {
      const t = Math.max(0, Math.min(duration, sec));
      const audio = audioRef.current;
      if (audio) audio.currentTime = t;
      setTime(t);
    },
    [duration],
  );

  const onKey = (e: React.KeyboardEvent) => {
    const step = e.shiftKey ? 5 : 1;
    const map: Record<string, number> = {
      ArrowRight: time + step,
      ArrowUp: time + step,
      ArrowLeft: time - step,
      ArrowDown: time - step,
      Home: 0,
      End: duration,
    };
    if (e.key in map) {
      e.preventDefault();
      seek(map[e.key]);
    }
  };

  const onBarsClick = (e: React.MouseEvent<HTMLDivElement>) => {
    const rect = e.currentTarget.getBoundingClientRect();
    if (rect.width <= 0) return;
    seek(((e.clientX - rect.left) / rect.width) * duration);
  };

  const peaks = load.status === "ready" ? resamplePeaks(load.wave.peaks, bars) : new Array<number>(bars).fill(0);
  const progress = duration > 0 ? time / duration : 0;

  return (
    <div
      style={{
        display: "flex",
        alignItems: "center",
        gap: 16,
        padding: "12px 16px",
        background: "var(--surface-2)",
        border: "1px solid var(--border)",
        borderRadius: 14,
        minWidth: 0,
      }}
    >
      <button
        type="button"
        className="tt-audio-play"
        aria-label={isPlaying ? "Pause recording" : "Play recording"}
        disabled={load.status !== "ready"}
        onClick={() => void toggle()}
        style={{
          width: 36,
          height: 36,
          flex: "none",
          borderRadius: "50%",
          border: "none",
          background: "var(--text)",
          display: "grid",
          placeItems: "center",
          cursor: load.status === "ready" ? "pointer" : "default",
          opacity: load.status === "ready" ? 1 : 0.4,
        }}
      >
        <Icon
          name={isPlaying ? "pause" : "play"}
          size={15}
          color="var(--surface)"
          fill="var(--surface)"
          style={isPlaying ? undefined : { marginLeft: 2 }}
        />
      </button>
      {load.status === "error" ? (
        <div role="alert" style={{ flex: 1, fontSize: 13, color: "var(--danger)" }}>
          Could not load this recording.
        </div>
      ) : (
        <div
          role="slider"
          tabIndex={0}
          aria-label="Seek"
          aria-valuemin={0}
          aria-valuemax={Math.round(duration * 10) / 10}
          aria-valuenow={Math.round(time * 10) / 10}
          aria-valuetext={`${clockLabel(time)} of ${clockLabel(duration)}`}
          aria-disabled={load.status !== "ready"}
          ref={barsRef}
          className="tt-audio-bars"
          onKeyDown={onKey}
          onClick={onBarsClick}
          style={{
            flex: 1,
            minWidth: 0,
            height: 36,
            display: "flex",
            alignItems: "center",
            justifyContent: "space-between",
            gap: 2,
            cursor: "pointer",
            borderRadius: 6,
            // Belt and braces: bars never paint past the row.
            overflow: "hidden",
          }}
        >
          {peaks.map((p, i) => {
            const played = (i + 0.5) / peaks.length <= progress;
            const dot = p < DOT_THRESHOLD;
            return (
              <span
                key={i}
                data-bar=""
                style={{
                  flex: "1 1 0",
                  maxWidth: dot ? 3 : 4,
                  minWidth: 2,
                  height: dot ? 3 : `${Math.max(12, Math.round(p * 100))}%`,
                  borderRadius: 999,
                  background: played ? "var(--text-secondary)" : "var(--border-strong)",
                }}
              />
            );
          })}
        </div>
      )}
      <span
        style={{
          flex: "none",
          fontSize: 13,
          color: "var(--text-secondary)",
          fontVariantNumeric: "tabular-nums",
          whiteSpace: "nowrap",
        }}
      >
        {clockLabel(time)} / {clockLabel(duration)}
      </span>
      {load.status === "ready" && (
        <audio
          ref={audioRef}
          src={load.url}
          preload="auto"
          onPlay={() => setIsPlaying(true)}
          onPause={() => setIsPlaying(false)}
          onEnded={() => {
            setIsPlaying(false);
            setTime(duration);
          }}
          onTimeUpdate={(e) => setTime(e.currentTarget.currentTime)}
        />
      )}
    </div>
  );
}
