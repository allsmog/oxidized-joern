#include "api.h"
void use(int amount) { void *pointer = reserve(amount); release(pointer); }
