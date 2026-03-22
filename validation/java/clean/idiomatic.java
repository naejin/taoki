// Expected: exit 0
// Expected: sections=imports,classes
// Expected: contains=UserRepository
// Expected: contains=findById
// Expected: contains=save

import java.util.HashMap;
import java.util.Map;
import java.util.Optional;

public class UserRepository {
    private final Map<Long, String> store = new HashMap<>();

    public Optional<String> findById(Long id) {
        return Optional.ofNullable(store.get(id));
    }

    public void save(Long id, String name) {
        store.put(id, name);
    }

    public void delete(Long id) {
        store.remove(id);
    }
}
