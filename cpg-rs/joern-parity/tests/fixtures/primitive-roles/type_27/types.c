unsigned long long identity(unsigned long long value) { unsigned long long local = value; return local; }
unsigned long long declared(unsigned long long value);
unsigned long long call_definition(unsigned long long value) { return identity(value); }
unsigned long long call_prototype(unsigned long long value) { return declared(value); }
