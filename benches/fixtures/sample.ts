import { EventEmitter } from "events";
import * as path from "path";
import axios, { AxiosInstance, AxiosResponse } from "axios";
import { z } from "zod";

export const MAX_RETRIES = 3;
export const DEFAULT_TIMEOUT_MS = 5000;

let globalInstance: ClientService | null = null;

export interface ClientConfig {
  baseUrl: string;
  timeoutMs?: number;
  maxRetries?: number;
  headers?: Record<string, string>;
  userAgent?: string;
}

export interface Indexable {
  id: string;
  kind: string;
  score(): number;
}

export interface PaginatedResult<T> {
  items: T[];
  total: number;
  page: number;
  perPage: number;
}

export enum Role {
  Admin = "admin",
  Editor = "editor",
  Viewer = "viewer",
}

export type ServiceErrorCode = "NOT_FOUND" | "UNAUTHORIZED" | "INVALID_INPUT" | "INTERNAL";

export class ServiceError extends Error {
  constructor(
    message: string,
    public readonly code: ServiceErrorCode,
    public readonly status: number = 500
  ) {
    super(message);
    this.name = "ServiceError";
  }
}

export class User implements Indexable {
  constructor(
    public readonly id: string,
    public readonly name: string,
    public readonly email: string,
    public readonly role: Role = Role.Viewer
  ) {}

  kind = "user" as const;

  score(): number {
    return this.role === Role.Admin ? 1.0 : 0.5;
  }

  toJSON(): Record<string, unknown> {
    return { id: this.id, name: this.name, email: this.email, role: this.role };
  }

  static fromJSON(data: Record<string, unknown>): User {
    return new User(
      data.id as string,
      data.name as string,
      data.email as string,
      (data.role as Role) ?? Role.Viewer
    );
  }
}

export class ClientService extends EventEmitter {
  private readonly client: AxiosInstance;

  constructor(private readonly config: ClientConfig) {
    super();
    this.client = axios.create({
      baseURL: config.baseUrl,
      timeout: config.timeoutMs ?? DEFAULT_TIMEOUT_MS,
      headers: config.headers,
    });
  }

  async fetchUser(id: string): Promise<User> {
    if (!id) throw new ServiceError("id cannot be empty", "INVALID_INPUT", 400);
    const resp: AxiosResponse = await this.client.get(`/users/${id}`);
    return User.fromJSON(resp.data);
  }

  async createUser(data: Omit<User, "id" | "score" | "kind" | "toJSON">): Promise<User> {
    const resp = await this.client.post("/users", data);
    return User.fromJSON(resp.data);
  }
}

export function paginate<T>(items: T[], page = 1, perPage = 20): PaginatedResult<T> {
  const total = items.length;
  const start = (page - 1) * perPage;
  return { items: items.slice(start, start + perPage), total, page, perPage };
}

export function parseHeader(raw: string): [string, string] | null {
  const idx = raw.indexOf(":");
  if (idx === -1) return null;
  return [raw.slice(0, idx).trim(), raw.slice(idx + 1).trim()];
}

function internalHash(data: Uint8Array): bigint {
  let h = BigInt("0xcbf29ce484222325");
  for (const b of data) {
    h ^= BigInt(b);
    h = (h * BigInt("0x100000001b3")) & BigInt("0xffffffffffffffff");
  }
  return h;
}

const UserSchema = z.object({
  id: z.string(),
  name: z.string(),
  email: z.string().email(),
  role: z.nativeEnum(Role).optional(),
});

globalInstance = new ClientService({ baseUrl: "http://localhost:8080" });

describe("ClientService", () => {
  it("should throw on empty id", async () => {
    const svc = new ClientService({ baseUrl: "http://localhost" });
    await expect(svc.fetchUser("")).rejects.toThrow("id cannot be empty");
  });

  it("paginate returns correct slice", () => {
    const items = Array.from({ length: 10 }, (_, i) => i);
    const result = paginate(items, 2, 3);
    expect(result.items).toEqual([3, 4, 5]);
  });
});
