"use strict";

const EventEmitter = require("events");
const path = require("path");
const axios = require("axios");

const MAX_RETRIES = 3;
const DEFAULT_TIMEOUT_MS = 5000;

let globalInstance = null;

/**
 * @typedef {Object} ClientConfig
 * @property {string} baseUrl
 * @property {number} [timeoutMs]
 * @property {number} [maxRetries]
 * @property {Record<string, string>} [headers]
 */

/**
 * Custom service error.
 */
class ServiceError extends Error {
  constructor(message, code = "INTERNAL", status = 500) {
    super(message);
    this.name = "ServiceError";
    this.code = code;
    this.status = status;
  }
}

/**
 * A simple user model.
 */
class User {
  constructor(id, name, email, role = "viewer") {
    this.id = id;
    this.name = name;
    this.email = email;
    this.role = role;
  }

  toJSON() {
    return { id: this.id, name: this.name, email: this.email, role: this.role };
  }

  static fromJSON(data) {
    return new User(data.id, data.name, data.email, data.role ?? "viewer");
  }
}

/**
 * HTTP client wrapping axios.
 */
class ClientService extends EventEmitter {
  constructor(config) {
    super();
    this.config = config;
    this.client = axios.create({
      baseURL: config.baseUrl,
      timeout: config.timeoutMs ?? DEFAULT_TIMEOUT_MS,
      headers: config.headers ?? {},
    });
  }

  async fetchUser(id) {
    if (!id) throw new ServiceError("id cannot be empty", "INVALID_INPUT", 400);
    const resp = await this.client.get(`/users/${id}`);
    return User.fromJSON(resp.data);
  }

  async createUser(data) {
    const resp = await this.client.post("/users", data);
    return User.fromJSON(resp.data);
  }
}

/**
 * Paginate an array of items.
 * @param {Array} items
 * @param {number} page
 * @param {number} perPage
 */
function paginate(items, page = 1, perPage = 20) {
  const total = items.length;
  const start = (page - 1) * perPage;
  return { items: items.slice(start, start + perPage), total, page, perPage };
}

/**
 * Parse a raw header string into [key, value].
 * @param {string} raw
 */
function parseHeader(raw) {
  const idx = raw.indexOf(":");
  if (idx === -1) return null;
  return [raw.slice(0, idx).trim(), raw.slice(idx + 1).trim()];
}

function _internalHash(data) {
  let h = BigInt("0xcbf29ce484222325");
  for (const b of data) {
    h ^= BigInt(b);
    h = (h * BigInt("0x100000001b3")) & BigInt("0xffffffffffffffff");
  }
  return h;
}

const app = require("express")();

app.use(require("express").json());

app.get("/users/:id", async (req, res) => {
  try {
    const svc = globalInstance;
    const user = await svc.fetchUser(req.params.id);
    res.json(user.toJSON());
  } catch (err) {
    res.status(err.status ?? 500).json({ error: err.message });
  }
});

app.post("/users", async (req, res) => {
  const svc = globalInstance;
  const user = await svc.createUser(req.body);
  res.status(201).json(user.toJSON());
});

globalInstance = new ClientService({ baseUrl: "http://localhost:8080" });

module.exports = { ServiceError, User, ClientService, paginate, parseHeader };
