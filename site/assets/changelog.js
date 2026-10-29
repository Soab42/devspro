// Renders changelog.json, which scripts/release.mjs generates from
// CHANGELOG.md. Text is escaped first; only `code` and **bold** are styled.
(function () {
  "use strict";

  var config = window.DEVSPRO_SITE || {};
  var MONTHS = ["January", "February", "March", "April", "May", "June", "July",
    "August", "September", "October", "November", "December"];

  function escapeHtml(text) {
    return String(text)
      .replace(/&/g, "&amp;")
      .replace(/</g, "&lt;")
      .replace(/>/g, "&gt;")
      .replace(/"/g, "&quot;")
      .replace(/'/g, "&#39;");
  }

  function inline(text) {
    return escapeHtml(text)
      .replace(/`([^`]+)`/g, "<code>$1</code>")
      .replace(/\*\*([^*]+)\*\*/g, "<strong>$1</strong>");
  }

  function isNext(release) {
    return !release.date || /^unreleased$/i.test(String(release.version));
  }

  // "v0.2.0" for releases, "next" for the unreleased section.
  function anchor(release) {
    return isNext(release) ? "next" : "v" + String(release.version).replace(/[^0-9A-Za-z.+-]/g, "");
  }

  function formatDate(iso) {
    var m = /^(\d{4})-(\d{2})-(\d{2})$/.exec(String(iso || ""));
    if (!m) return "";
    return Number(m[3]) + " " + MONTHS[Number(m[2]) - 1] + " " + m[1];
  }

  function render(data) {
    var releases = (data && Array.isArray(data.releases) ? data.releases : []).filter(function (r) {
      return r && r.version;
    });
    var host = document.querySelector("[data-releases]");
    var tocList = document.querySelector("[data-toc-list]");
    if (!host) return;

    if (!releases.length) {
      host.innerHTML = '<p class="state">No releases yet.</p>';
      return;
    }

    var firstReleased = -1;
    for (var i = 0; i < releases.length; i++) {
      if (!isNext(releases[i])) {
        firstReleased = i;
        break;
      }
    }

    var html = "";
    var toc = "";
    releases.forEach(function (release, index) {
      var next = isNext(release);
      var id = anchor(release);
      var title = next ? "Coming next" : escapeHtml(release.version);
      var date = next ? "" : formatDate(release.date);

      html += '<article class="release' + (next ? " is-next" : "") + '" id="' + id + '" aria-labelledby="' + id + '-title">';
      html += '<header class="release-head">';
      html += '<h2 id="' + id + '-title"><a href="#' + id + '">' + title + "</a></h2>";
      if (next) html += '<span class="tag tag-next">In progress</span>';
      if (index === firstReleased) html += '<span class="tag tag-latest">Latest</span>';
      if (date) html += '<time datetime="' + escapeHtml(release.date) + '">' + date + "</time>";
      html += "</header>";

      (Array.isArray(release.groups) ? release.groups : []).forEach(function (group) {
        var items = Array.isArray(group.items) ? group.items : [];
        if (!items.length) return;
        html += '<section class="release-group">';
        if (group.title) html += "<h3>" + escapeHtml(group.title) + "</h3>";
        html += "<ul>" + items.map(function (item) {
          return "<li>" + inline(item) + "</li>";
        }).join("") + "</ul>";
        html += "</section>";
      });

      if (index === firstReleased) {
        html += '<div class="release-foot"><a class="btn btn-secondary btn-sm" href="./#download">' +
          '<svg class="i" aria-hidden="true"><use href="assets/icons.svg#download"/></svg>Download ' +
          escapeHtml(release.version) + "</a></div>";
      }
      html += "</article>";

      toc += '<li><a href="#' + id + '">' + (next ? "Next" : escapeHtml(release.version)) + "</a></li>";
    });

    host.innerHTML = html;
    if (tocList) {
      tocList.innerHTML = toc;
      var nav = document.querySelector("[data-toc]");
      if (nav) nav.hidden = false;
    }

    // The content arrived after the browser tried to scroll to #v0.2.0.
    if (location.hash) {
      var target = document.getElementById(decodeURIComponent(location.hash.slice(1)));
      if (target) target.scrollIntoView();
    }
  }

  function fail() {
    var host = document.querySelector("[data-releases]");
    var releases = (config.repo || "https://github.com/Soab42/devspro").replace(/\/$/, "") + "/releases";
    if (host) {
      host.innerHTML = '<p class="state">The release notes didn\'t load. They are also on ' +
        '<a href="' + escapeHtml(releases) + '">GitHub Releases</a>.</p>';
    }
  }

  function init() {
    if (config.repo) {
      Array.prototype.forEach.call(document.querySelectorAll("[data-releases-link]"), function (a) {
        a.href = config.repo.replace(/\/$/, "") + "/releases";
      });
    }
    if (!window.fetch) return fail();
    fetch("changelog.json", { cache: "no-store" })
      .then(function (r) {
        if (!r.ok) throw new Error(r.status);
        return r.json();
      })
      .then(render)
      .catch(fail);
  }

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", init);
  } else {
    init();
  }
})();
