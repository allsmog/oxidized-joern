#include "defs.h"
int source(int value);
void sink(int value);
void probe(int value) {
  value = ID(
      source(
          value
      )
  );
  sink(value);
  value = source(value);
  sink(value);
}
