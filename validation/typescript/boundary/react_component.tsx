// Expected: exit 0
// Expected: contains=fns:
// Expected: contains=App
// Expected: contains=UserCard

import React from 'react';

interface UserCardProps {
    name: string;
    email: string;
}

export function UserCard({ name, email }: UserCardProps) {
    return <div>{name} ({email})</div>;
}

export default function App() {
    return <UserCard name="Test" email="test@example.com" />;
}
