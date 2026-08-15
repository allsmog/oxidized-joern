#include <stdlib.h>

struct record {
    int left;
    int right;
};

struct pointer_box {
    int *value;
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

int pointer_to_pointer(int value) {
    int result = 0;
    int *cursor = &result;
    int **slot = &cursor;
    **slot = value;
    return result;
}

static int *return_alias(int *value) {
    return value;
}

int returned_alias(int value) {
    int result = 0;
    int *cursor = return_alias(&result);
    *cursor = value;
    return result;
}

int pointer_field_alias(int value) {
    int result = 0;
    struct pointer_box box;
    box.value = &result;
    *box.value = value;
    return result;
}

int pointer_rebind(int value) {
    int first = 0;
    int second = 0;
    int *cursor = &first;
    cursor = &second;
    *cursor = value;
    return second - first;
}

int aliased_function_pointer(int value) {
    int (*callback)(int) = increment;
    int (*alias)(int) = callback;
    return alias(value);
}

static void store_through_pointer(int *output, int value) {
    *output = value;
}

int out_parameter_alias(int value) {
    int result = 0;
    store_through_pointer(&result, value);
    return result;
}
