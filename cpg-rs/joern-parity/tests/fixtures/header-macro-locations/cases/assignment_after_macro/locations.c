#include "defs.h"
void sink(int value);
void probe(int value) {
  STEP(value);
  value = 17;
  sink(value);
  sink(17);
}
