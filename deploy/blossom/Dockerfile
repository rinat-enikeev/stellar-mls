FROM node:20-alpine AS builder

RUN apk add --no-cache git

WORKDIR /app
RUN git clone --depth 1 https://github.com/hzrd149/blossom-server.git .
RUN npm install
RUN npm run build 2>/dev/null || true

FROM node:20-alpine

WORKDIR /app
COPY --from=builder /app /app

ENV DATA_DIR=/app/data
ENV PORT=3000

RUN mkdir -p /app/data

EXPOSE 3000

CMD ["node", "src/index.js"]
