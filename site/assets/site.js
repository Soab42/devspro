// Shared page behaviour: the release feed, the Linux / Windows install tabs,
// copy buttons, the Product Hunt banner and screenshot swaps. Every part
// degrades to the static HTML.
(function () {
  "use strict";

  var config = window.DEVSPRO_SITE || {};
  var FALLBACK = config.releasesFallback || "https://github.com/Soab42/devspro/releases/latest";
  // Keys of latest.json's "platforms", by package kind.
  var PLATFORMS = { deb: "linux-x86_64-deb", rpm: "linux-x86_64-rpm", exe: "windows-x86_64-nsis" };
  var INSTALL_PREFIX = { deb: "sudo apt install ./", rpm: "sudo dnf install ./", exe: "" };

  function each(selector, fn) {
    Array.prototype.forEach.call(document.querySelectorAll(selector), fn);
  }

  function fileName(url) {
    try {
      var path = new URL(url, location.href).pathname;
      return decodeURIComponent(path.slice(path.lastIndexOf("/") + 1));
    } catch (e) {
      return "";
    }
  }

  // ------------------------------------------------------------ the feed --
  // latest.json is the same feed the app's updater reads.
  function applyFeed(feed) {
    var version = feed && typeof feed.version === "string" ? feed.version : "";
    var platforms = (feed && feed.platforms) || {};

    if (version) {
      each("[data-version]", function (el) {
        el.textContent = "v" + version;
      });
      each("[data-version-label]", function (el) {
        el.textContent = "v" + version + " is out";
      });
    }

    Object.keys(PLATFORMS).forEach(function (kind) {
      var entry = platforms[PLATFORMS[kind]];
      var url = entry && typeof entry.url === "string" && entry.url ? entry.url : "";
      each('[data-download="' + kind + '"]', function (el) {
        el.href = url || FALLBACK;
      });
      // Names come only from a real URL: a package missing from the feed may
      // not exist for this version, so its wildcard text stays.
      var name = url ? fileName(url) : "";
      if (name) {
        each('[data-install="' + kind + '"]', function (el) {
          el.textContent = INSTALL_PREFIX[kind] + name;
        });
      }
      if (name) {
        each('[data-filename="' + kind + '"]', function (el) {
          el.textContent = name;
        });
      }
    });
  }

  function loadFeed() {
    if (!document.querySelector("[data-download], [data-version], [data-version-label]")) return;
    if (!window.fetch) return;
    fetch("latest.json", { cache: "no-store" })
      .then(function (r) {
        if (!r.ok) throw new Error(r.status);
        return r.json();
      })
      .then(applyFeed)
      .catch(function () {
        // Leave the static fallbacks in place.
      });
  }

  // -------------------------------------------------------- install tabs --
  // WAI-ARIA tabs: arrow keys, Home and End move between them. Opens on the
  // visitor's system (data-os, set in <head>), else Linux.
  function wireOsTabs() {
    var list = document.querySelector("[data-os-tabs]");
    if (!list) return;
    var tabs = Array.prototype.slice.call(list.querySelectorAll("[data-os-tab]"));
    var panels = {};
    tabs.forEach(function (tab) {
      panels[tab.getAttribute("data-os-tab")] = document.getElementById(tab.getAttribute("aria-controls"));
    });

    function select(tab, focus) {
      tabs.forEach(function (t) {
        var on = t === tab;
        t.setAttribute("aria-selected", on ? "true" : "false");
        t.tabIndex = on ? 0 : -1;
        var panel = panels[t.getAttribute("data-os-tab")];
        if (panel) panel.hidden = !on;
      });
      if (focus) tab.focus();
    }

    tabs.forEach(function (tab, i) {
      tab.addEventListener("click", function () {
        select(tab, false);
      });
      tab.addEventListener("keydown", function (e) {
        var next = null;
        if (e.key === "ArrowRight") next = tabs[(i + 1) % tabs.length];
        else if (e.key === "ArrowLeft") next = tabs[(i - 1 + tabs.length) % tabs.length];
        else if (e.key === "Home") next = tabs[0];
        else if (e.key === "End") next = tabs[tabs.length - 1];
        if (next) {
          e.preventDefault();
          select(next, true);
        }
      });
    });

    var os = document.documentElement.getAttribute("data-os");
    var initial = tabs.filter(function (t) { return t.getAttribute("data-os-tab") === os; })[0] || tabs[0];
    list.hidden = false;
    document.documentElement.classList.add("tabs-ready");
    select(initial, false);
  }

  // -------------------------------------------------------- copy buttons --
  function copyText(text) {
    if (navigator.clipboard && window.isSecureContext) {
      return navigator.clipboard.writeText(text);
    }
    return new Promise(function (resolve, reject) {
      var area = document.createElement("textarea");
      area.value = text;
      area.setAttribute("readonly", "");
      area.style.position = "fixed";
      area.style.opacity = "0";
      document.body.appendChild(area);
      area.select();
      try {
        document.execCommand("copy") ? resolve() : reject(new Error("copy failed"));
      } catch (e) {
        reject(e);
      } finally {
        document.body.removeChild(area);
      }
    });
  }

  function wireCopyButtons() {
    each("[data-copy]", function (btn) {
      btn.addEventListener("click", function () {
        var target = document.querySelector('[data-install="' + btn.getAttribute("data-copy") + '"]');
        if (!target) return;
        var label = btn.querySelector("span");
        copyText(target.textContent.trim()).then(
          function () {
            btn.classList.add("is-done");
            if (label) label.textContent = "Copied";
            setTimeout(function () {
              btn.classList.remove("is-done");
              if (label) label.textContent = "Copy";
            }, 1800);
          },
          function () {
            if (label) label.textContent = "Select it";
          }
        );
      });
    });
  }

  // -------------------------------------------------------- Product Hunt --
  function productHunt() {
    var ph = config.productHunt || {};
    var url = typeof ph.url === "string" ? ph.url.trim() : "";
    if (!url) return;

    each("[data-ph-link]", function (a) {
      a.href = url;
      a.target = "_blank";
      a.rel = "noopener";
    });

    var banner = document.querySelector("[data-ph-banner]");
    var text = document.querySelector("[data-ph-text]");
    if (banner && ph.live) {
      banner.classList.add("is-visible");
    } else if (banner && text && ph.launchDate) {
      text.textContent = "DevsPro launches on Product Hunt on " + ph.launchDate + ".";
      var cta = banner.querySelector(".ph-banner-cta");
      if (cta) cta.textContent = "Get notified";
      banner.classList.add("is-visible");
    }

    var postId = typeof ph.postId === "string" ? ph.postId.trim() : String(ph.postId || "");
    var badge = document.querySelector("[data-ph-badge]");
    var img = document.querySelector("[data-ph-badge-img]");
    if (postId && badge && img) {
      var dark = !window.matchMedia || !window.matchMedia("(prefers-color-scheme: light)").matches;
      img.src =
        "https://api.producthunt.com/widgets/embed-image/v1/featured.svg?post_id=" +
        encodeURIComponent(postId) +
        "&theme=" + (dark ? "dark" : "light");
      badge.classList.add("is-visible");
    }
  }

  // --------------------------------------------------------- screenshots --
  function screenshots() {
    var shots = config.screenshots || {};
    each("[data-shot]", function (el) {
      var src = shots[el.getAttribute("data-shot")];
      if (!src) return;
      var img = new Image();
      img.className = "shot-img";
      img.alt = el.getAttribute("data-shot-alt") || "DevsPro screenshot";
      img.decoding = "async";
      img.onload = function () {
        // Keep a figure's caption element, but it no longer describes an illustration.
        var caption = el.querySelector("figcaption, .panel-caption");
        el.textContent = "";
        el.appendChild(img);
        if (caption && el.tagName === "FIGURE") {
          caption.textContent = img.alt;
          el.appendChild(caption);
        }
      };
      img.src = src;
    });
  }

  // ------------------------------------------------------------- misc --
  function links() {
    if (config.repo) {
      each("[data-repo-link]", function (a) {
        a.href = config.repo;
      });
      each("[data-license-link]", function (a) {
        a.href = config.repo.replace(/\/$/, "") + "/blob/main/LICENSE";
      });
    }
    each("[data-year]", function (el) {
      el.textContent = String(new Date().getFullYear());
    });
  }

  function init() {
    links();
    loadFeed();
    wireOsTabs();
    wireCopyButtons();
    productHunt();
    screenshots();
  }

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", init);
  } else {
    init();
  }
})();
