unsigned long target(unsigned long value);
unsigned long for_callbacks(unsigned long value) { for (unsigned short (*target)(unsigned short) = 0; value; value = 0) { target(value); } return target(value); }
