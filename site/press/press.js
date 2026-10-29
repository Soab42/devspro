// Press templates borrow their illustrations from the landing page, so the
// images and the site never drift apart. Served over HTTP by render.sh.
window.pressPull = function (pairs) {
  return fetch("../index.html")
    .then(function (r) { return r.text(); })
    .then(function (html) {
      var doc = new DOMParser().parseFromString(html, "text/html");
      pairs.forEach(function (pair) {
        var src = doc.querySelector(pair[0]);
        var el = document.querySelector(pair[1]);
        if (src && el) {
          // Our own page's markup; only its relative asset paths need fixing.
          el.innerHTML = src.outerHTML.replace(/(href|src)="assets\//g, '$1="../assets/');
        }
      });
      document.documentElement.setAttribute("data-ready", "1");
    });
};
