unsigned long declared(unsigned long value);
unsigned long defined(unsigned long value) { return value; }
unsigned long paren_calls(unsigned long value) { return (declared)(value) + (defined)(value); }
