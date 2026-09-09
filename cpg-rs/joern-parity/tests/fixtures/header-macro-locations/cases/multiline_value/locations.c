#include "defs.h"
int source(int value);
void sink(int value);
void probe(int value) {
  value = DOUBLE(
      value
  );
  sink(value);
  value = 9;
  sink(value);
}
