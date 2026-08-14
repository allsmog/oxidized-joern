#include <stdlib.h>

struct record {
    int left;
    int right;
};

static int increment(int value) {
    return value + 1;
}

int function_pointer(int value) {
    int (*callback)(int) = increment;
    return callback(value);
}

int heap_alias(int value) {
    struct record *heap = malloc(sizeof(struct record));
    struct record *alias = heap;
    alias->left = value;
    heap->right = alias->left + 1;
    int result = heap->right;
    free(heap);
    return result;
}

int array_alias(int value) {
    int values[2];
    int *cursor = values;
    cursor[0] = value;
    values[1] = cursor[0] + 1;
    return values[1];
}
