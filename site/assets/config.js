// Site switches. Edit, commit and push to main; the Pages workflow redeploys.
// Everything here is optional: with the defaults the site works as is.
window.DEVSPRO_SITE = {
  repo: "https://github.com/Soab42/devspro",
  // Where the download buttons go when latest.json has no package for a format.
  releasesFallback: "https://github.com/Soab42/devspro/releases/latest",

  productHunt: {
    // Launch day: set live to true and url to the post. A banner appears on
    // every page.
    live: false,
    url: "",
    // From the post's embed code (post_id=...). Shows the official
    // "Featured on Product Hunt" badge under the hero buttons.
    postId: "",
    // Before launch: with live still false, a url and a date such as
    // "12 November" show "DevsPro launches on Product Hunt on 12 November".
    launchDate: "",
  },

  // Real screenshots. Put PNGs in assets/screenshots/ and name them here;
  // each one replaces the matching illustration (data-shot="...") on the page.
  // Example: hero: "assets/screenshots/dashboard.png"
  screenshots: {
    hero: "",
    wizard: "",
    cleaner: "",
    agents: "",
  },
};
