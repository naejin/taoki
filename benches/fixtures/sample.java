package com.example.service;

import java.util.ArrayList;
import java.util.HashMap;
import java.util.List;
import java.util.Map;
import java.util.Optional;
import java.util.concurrent.CompletableFuture;
import java.net.http.HttpClient;
import java.net.http.HttpRequest;
import java.net.http.HttpResponse;
import java.net.URI;
import java.time.Duration;

/**
 * Enumeration of user roles.
 */
public enum Role {
    ADMIN,
    EDITOR,
    VIEWER;

    public boolean canEdit() {
        return this == ADMIN || this == EDITOR;
    }
}

/**
 * Interface for indexable domain objects.
 */
public interface Indexable {
    String getId();
    String getKind();
    double getScore();
}

/**
 * Interface for serializable objects.
 */
public interface Serializable {
    Map<String, Object> toMap();
    String toJson();
}

/**
 * Checked exception for service-layer errors.
 */
public class ServiceException extends Exception {
    private final String code;
    private final int status;

    public ServiceException(String message, String code, int status) {
        super(message);
        this.code = code;
        this.status = status;
    }

    public String getCode() { return code; }
    public int getStatus() { return status; }
}

/**
 * Configuration record for the HTTP client.
 */
public record ClientConfig(
    String baseUrl,
    int timeoutMs,
    int maxRetries,
    Map<String, String> headers,
    String userAgent,
    boolean followRedirects,
    boolean verifySsl
) {
    public static ClientConfig defaultConfig(String baseUrl) {
        return new ClientConfig(baseUrl, 5000, 3, new HashMap<>(), "taoki/0.1", true, true);
    }
}

/**
 * Domain entity representing an application user.
 */
public class User implements Indexable, Serializable {
    private final String id;
    private final String name;
    private final String email;
    private final Role role;

    public User(String id, String name, String email, Role role) {
        this.id = id;
        this.name = name;
        this.email = email;
        this.role = role != null ? role : Role.VIEWER;
    }

    @Override
    public String getId() { return id; }

    @Override
    public String getKind() { return "user"; }

    @Override
    public double getScore() { return role == Role.ADMIN ? 1.0 : 0.5; }

    @Override
    public Map<String, Object> toMap() {
        Map<String, Object> m = new HashMap<>();
        m.put("id", id);
        m.put("name", name);
        m.put("email", email);
        m.put("role", role.name().toLowerCase());
        return m;
    }

    @Override
    public String toJson() {
        return String.format(
            "{\"id\":\"%s\",\"name\":\"%s\",\"email\":\"%s\",\"role\":\"%s\"}",
            id, name, email, role.name().toLowerCase()
        );
    }

    public static User fromMap(Map<String, Object> data) {
        return new User(
            (String) data.get("id"),
            (String) data.get("name"),
            (String) data.get("email"),
            Role.valueOf(((String) data.getOrDefault("role", "viewer")).toUpperCase())
        );
    }
}

/**
 * HTTP-backed client service.
 */
public class ClientService {
    private final ClientConfig config;
    private final HttpClient httpClient;

    public ClientService(ClientConfig config) {
        this.config = config;
        this.httpClient = HttpClient.newBuilder()
            .connectTimeout(Duration.ofMillis(config.timeoutMs()))
            .build();
    }

    public CompletableFuture<User> fetchUser(String id) {
        if (id == null || id.isEmpty()) {
            return CompletableFuture.failedFuture(
                new ServiceException("id cannot be empty", "INVALID_INPUT", 400)
            );
        }
        HttpRequest req = HttpRequest.newBuilder()
            .uri(URI.create(config.baseUrl() + "/users/" + id))
            .GET()
            .build();
        return httpClient.sendAsync(req, HttpResponse.BodyHandlers.ofString())
            .thenApply(resp -> new User(id, "Alice", "alice@example.com", Role.VIEWER));
    }

    public static <T> PaginatedResult<T> paginate(List<T> items, int page, int perPage) {
        int total = items.size();
        int start = Math.min((page - 1) * perPage, total);
        int end = Math.min(start + perPage, total);
        return new PaginatedResult<>(new ArrayList<>(items.subList(start, end)), total, page, perPage);
    }
}

/**
 * Generic paginated result container.
 */
public record PaginatedResult<T>(List<T> items, int total, int page, int perPage) {}
