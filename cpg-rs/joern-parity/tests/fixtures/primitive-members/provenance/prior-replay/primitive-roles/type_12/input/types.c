unsigned identity(unsigned value) { unsigned local = value; return local; }
unsigned declared(unsigned value);
unsigned call_definition(unsigned value) { return identity(value); }
unsigned call_prototype(unsigned value) { return declared(value); }
