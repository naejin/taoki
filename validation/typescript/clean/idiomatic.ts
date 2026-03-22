// Expected: exit 0
// Expected: sections=imports,types,fns
// Expected: contains=UserService
// Expected: contains=getUser
// Expected: contains=User

import { Database } from './db';

export interface User {
    id: number;
    name: string;
    email: string;
}

export class UserService {
    constructor(private db: Database) {}

    async getUser(id: number): Promise<User | null> {
        return this.db.findById('users', id);
    }

    async createUser(name: string, email: string): Promise<User> {
        return this.db.insert('users', { name, email });
    }
}

export function validateEmail(email: string): boolean {
    return email.includes('@');
}
