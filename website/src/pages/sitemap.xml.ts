import type { APIRoute } from 'astro';
import { releasesByLocale, PAGE_SIZE } from '~/content/changelog';

export const GET: APIRoute = ({ site }) => {
  const paths = ['/', '/guide/', '/installation/', '/agent/', '/privacy/', '/changelog/'];
  paths.push(...releasesByLocale.zh.map(release => `/changelog/${release.version}/`));
  for (let page = 2; page <= Math.ceil(releasesByLocale.zh.length / PAGE_SIZE); page++) paths.push(`/changelog/page/${page}/`);
  const urls = paths.map(path => `<url><loc>${new URL(path, site).href}</loc></url>`);
  return new Response(`<?xml version="1.0" encoding="UTF-8"?>\n<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">${urls.join('')}</urlset>`, {headers: {'Content-Type': 'application/xml; charset=utf-8'}});
};
