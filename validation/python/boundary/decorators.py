# Expected: exit 0
# Expected: contains=fns:
# Expected: contains=my_view
# Expected: contains=MyModel

from functools import wraps


def require_auth(func):
    @wraps(func)
    def wrapper(*args, **kwargs):
        return func(*args, **kwargs)
    return wrapper


@require_auth
def my_view(request):
    return {"status": "ok"}


class MyModel:
    class Meta:
        table_name = "my_model"

    def save(self):
        pass
