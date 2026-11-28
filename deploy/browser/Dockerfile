# Playwright image (Chromium and its system libraries), pinned by digest.
# The npm package `playwright` in package.json must stay on the same version.
FROM mcr.microsoft.com/playwright:v1.65.1-noble@sha256:1507cbfc550a63a6b7396791ca4f00df17beb56342d2447b3f106c306f525579

WORKDIR /app
ENV NODE_ENV=production \
    PLAYWRIGHT_SKIP_BROWSER_DOWNLOAD=1 \
    HOME=/tmp \
    BROWSER_PORT=3000

COPY package.json package-lock.json ./
RUN npm ci --omit=dev --ignore-scripts && npm cache clean --force

COPY src ./src
COPY test ./test
COPY LICENSE-NOTICE ./

# Numeric user: the compose file runs it read-only with all capabilities dropped.
USER 1000:1000
EXPOSE 3000

HEALTHCHECK --interval=30s --timeout=5s --start-period=20s --retries=3 \
  CMD node -e "fetch('http://127.0.0.1:'+(process.env.BROWSER_PORT||3000)+'/v1/health').then(r=>process.exit(r.ok?0:1),()=>process.exit(1))"

CMD ["node", "src/server.mjs"]
