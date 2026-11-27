/* Teletype marketing site — small, dependency-free interactions. */
(function () {
  "use strict";

  var root = document.documentElement;
  var reduceMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;

  /* ---------- theme ---------- */
  var THEME_KEY = "teletype-theme";
  var stored = null;
  try { stored = localStorage.getItem(THEME_KEY); } catch (e) { /* private mode */ }
  var initial = stored || (window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light");
  root.setAttribute("data-theme", initial);

  var themeToggle = document.querySelector("[data-theme-toggle]");
  if (themeToggle) {
    themeToggle.addEventListener("click", function () {
      var next = root.getAttribute("data-theme") === "dark" ? "light" : "dark";
      root.setAttribute("data-theme", next);
      try { localStorage.setItem(THEME_KEY, next); } catch (e) { /* ignore */ }
      themeToggle.setAttribute("aria-label", next === "dark" ? "Switch to light theme" : "Switch to dark theme");
    });
  }

  /* ---------- mobile navigation ---------- */
  var navToggle = document.querySelector("[data-nav-toggle]");
  var nav = document.getElementById("site-navigation");
  if (navToggle && nav) {
    navToggle.addEventListener("click", function () {
      var open = nav.classList.toggle("is-open");
      navToggle.setAttribute("aria-expanded", String(open));
      navToggle.setAttribute("aria-label", open ? "Close navigation" : "Open navigation");
    });
    nav.addEventListener("click", function (event) {
      if (event.target.closest("a") && window.matchMedia("(max-width: 820px)").matches) {
        nav.classList.remove("is-open");
        navToggle.setAttribute("aria-expanded", "false");
      }
    });
  }

  /* ---------- features menu ---------- */
  var menuParent = document.querySelector("[data-menu]");
  var menuTrigger = document.querySelector("[data-menu-trigger]");
  if (menuParent && menuTrigger) {
    menuTrigger.addEventListener("click", function (event) {
      event.preventDefault();
      var open = menuParent.classList.toggle("is-open");
      menuTrigger.setAttribute("aria-expanded", String(open));
    });
    document.addEventListener("click", function (event) {
      if (!menuParent.contains(event.target)) {
        menuParent.classList.remove("is-open");
        menuTrigger.setAttribute("aria-expanded", "false");
      }
    });
    document.addEventListener("keydown", function (event) {
      if (event.key === "Escape") {
        menuParent.classList.remove("is-open");
        menuTrigger.setAttribute("aria-expanded", "false");
      }
    });
  }

  /* ---------- seamless app marquee ---------- */
  var track = document.querySelector("[data-marquee]");
  if (track) {
    var list = track.querySelector(".compatible-list");
    if (list) {
      var clone = list.cloneNode(true);
      clone.setAttribute("aria-hidden", "true");
      track.appendChild(clone);
    }
  }

  /* ---------- hero demo ---------- */
  var draft = document.getElementById("hero-draft");
  var pill = document.querySelector("[data-pill]");
  var pillLabel = document.querySelector("[data-pill-label]");
  var steps = Array.prototype.slice.call(document.querySelectorAll("[data-hero-step]"));
  var replay = document.getElementById("hero-replay");
  var announcement = document.getElementById("hero-announcement");
  var finalHTML = draft ? draft.innerHTML : "";
  var heroTimers = [];
  var running = false;

  var LINES = [
    "Hi Maya,",
    "For Friday, please bring:",
    "\u2022 The design notes",
    "\u2022 The launch checklist",
    "\u2022 The revised budget of $6,239",
    "Thanks,",
    "Amira"
  ];

  function setPhase(phase) {
    if (pill) {
      pill.setAttribute("data-phase", phase);
      if (pillLabel) {
        pillLabel.textContent = phase === "listening" ? "Listening" : phase === "polish" ? "Transcribing" : "Pasted";
      }
    }
    steps.forEach(function (item) {
      item.classList.toggle("is-active", item.getAttribute("data-hero-step") === phase);
    });
  }

  function clearTimers() {
    heroTimers.forEach(clearTimeout);
    heroTimers = [];
  }

  function finishHero() {
    if (draft) draft.innerHTML = finalHTML;
    setPhase("done");
    running = false;
    if (announcement) announcement.textContent = "Dictation pasted into the Gmail draft.";
    if (replay) replay.disabled = false;
  }

  function typeDraft(done) {
    if (!draft) { done(); return; }
    draft.innerHTML = "";
    var lineIndex = 0;
    var charIndex = 0;
    var p = document.createElement("p");
    draft.appendChild(p);

    function step() {
      if (lineIndex >= LINES.length) { done(); return; }
      var line = LINES[lineIndex];
      if (charIndex < line.length) {
        p.textContent += line.charAt(charIndex);
        charIndex += 1;
        heroTimers.push(setTimeout(step, 14));
        return;
      }
      lineIndex += 1;
      charIndex = 0;
      if (lineIndex < LINES.length) {
        p = document.createElement("p");
        draft.appendChild(p);
        heroTimers.push(setTimeout(step, 95));
      } else {
        heroTimers.push(setTimeout(done, 120));
      }
    }
    step();
  }

  function playHero() {
    if (running || !draft) return;
    running = true;
    clearTimers();
    if (replay) replay.disabled = true;
    setPhase("listening");
    if (announcement) announcement.textContent = "Listening.";
    draft.innerHTML = "<p><span class=\"caret\"></span></p>";

    heroTimers.push(setTimeout(function () {
      setPhase("polish");
      if (announcement) announcement.textContent = "Transcribing on device.";
      heroTimers.push(setTimeout(function () {
        typeDraft(finishHero);
      }, 480));
    }, 1150));
  }

  if (draft) {
    if (reduceMotion) {
      setPhase("done");
      if (replay) replay.disabled = false;
    } else {
      heroTimers.push(setTimeout(playHero, 420));
      if (replay) replay.addEventListener("click", playHero);
    }
  }

  /* ---------- generic tab roving ---------- */
  function wireTabs(options) {
    var tabs = Array.prototype.slice.call(document.querySelectorAll(options.tabSelector));
    var panels = Array.prototype.slice.call(document.querySelectorAll(options.panelSelector));
    if (!tabs.length) return;
    var countEl = document.querySelector(options.countSelector);

    function select(index, focus) {
      index = (index + tabs.length) % tabs.length;
      tabs.forEach(function (tab, i) {
        var active = i === index;
        tab.setAttribute("aria-selected", String(active));
        tab.tabIndex = active ? 0 : -1;
        if (active && focus) tab.focus();
        if (active && options.scroll) tab.scrollIntoView({ block: "nearest", inline: "nearest", behavior: "smooth" });
      });
      panels.forEach(function (panel, i) { panel.hidden = i !== index; });
      if (countEl) countEl.textContent = options.countLabel(index + 1, tabs.length);
      options.onSelect && options.onSelect(index);
    }

    var selected = tabs.findIndex(function (tab) { return tab.getAttribute("aria-selected") === "true"; });
    select(selected < 0 ? 0 : selected, false);

    tabs.forEach(function (tab, i) {
      tab.addEventListener("click", function () { select(i, false); });
      tab.addEventListener("keydown", function (event) {
        var next = null;
        if (event.key === "ArrowRight" || event.key === "ArrowDown") next = i + 1;
        if (event.key === "ArrowLeft" || event.key === "ArrowUp") next = i - 1;
        if (event.key === "Home") next = 0;
        if (event.key === "End") next = tabs.length - 1;
        if (next !== null) { event.preventDefault(); select(next, true); }
      });
    });

    var prev = document.querySelector(options.prevSelector);
    var next = document.querySelector(options.nextSelector);
    if (prev) prev.addEventListener("click", function () {
      select(tabs.findIndex(function (t) { return t.getAttribute("aria-selected") === "true"; }) - 1, false);
    });
    if (next) next.addEventListener("click", function () {
      select(tabs.findIndex(function (t) { return t.getAttribute("aria-selected") === "true"; }) + 1, false);
    });
  }

  wireTabs({
    tabSelector: ".polish-tab",
    panelSelector: ".polish-card",
    countSelector: "[data-polish-count]",
    prevSelector: "[data-polish-prev]",
    nextSelector: "[data-polish-next]",
    scroll: true,
    countLabel: function (i, total) { return i + " of " + total; }
  });

  wireTabs({
    tabSelector: ".person-chip",
    panelSelector: ".person-card",
    countSelector: "[data-person-count]",
    prevSelector: "[data-person-prev]",
    nextSelector: "[data-person-next]",
    scroll: true,
    countLabel: function (i, total) { return i + " / " + total; }
  });

  /* ---------- copy to clipboard ---------- */
  var copyButton = document.querySelector("[data-copy]");
  if (copyButton) {
    copyButton.addEventListener("click", function () {
      var text = copyButton.getAttribute("data-copy");
      var done = function () {
        var old = copyButton.getAttribute("aria-label");
        copyButton.setAttribute("aria-label", "Copied");
        setTimeout(function () { copyButton.setAttribute("aria-label", old); }, 1600);
      };
      if (navigator.clipboard && navigator.clipboard.writeText) {
        navigator.clipboard.writeText(text).then(done, function () { /* ignore */ });
      }
    });
  }

  /* ---------- reveal on scroll ---------- */
  if (!reduceMotion && "IntersectionObserver" in window) {
    var revealTargets = document.querySelectorAll(
      ".section-heading, .hood-step, .hood-note, .engine-card, .hood-lane, .polish-panel, .person-card, .proof-card, .compare-card, .founder-copy, .download-closing h2"
    );
    var observer = new IntersectionObserver(function (entries) {
      entries.forEach(function (entry) {
        if (entry.isIntersecting) {
          entry.target.classList.add("is-in");
          observer.unobserve(entry.target);
        }
      });
    }, { rootMargin: "0px 0px -8% 0px", threshold: 0.06 });

    revealTargets.forEach(function (el) {
      if (el.closest("[hidden]")) return;
      el.classList.add("reveal");
      observer.observe(el);
    });
  }
})();
