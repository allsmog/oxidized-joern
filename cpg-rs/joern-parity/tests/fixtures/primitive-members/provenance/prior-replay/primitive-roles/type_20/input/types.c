long unsigned identity(long unsigned value) { long unsigned local = value; return local; }
long unsigned declared(long unsigned value);
long unsigned call_definition(long unsigned value) { return identity(value); }
long unsigned call_prototype(long unsigned value) { return declared(value); }
