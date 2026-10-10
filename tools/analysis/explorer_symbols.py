"""Read PE/PDB identifiers and relevant public symbols. Never loads code into Explorer."""
import argparse
import ctypes
import json
import struct
import urllib.request
import uuid
from pathlib import Path


def pe_metadata(path):
    data = Path(path).read_bytes()
    pe = struct.unpack_from('<I', data, 0x3C)[0]
    assert data[pe:pe+4] == b'PE\0\0'
    sections_count = struct.unpack_from('<H', data, pe+6)[0]
    optional_size = struct.unpack_from('<H', data, pe+20)[0]
    optional = pe+24
    is64 = struct.unpack_from('<H', data, optional)[0] == 0x20B
    directories = optional+(112 if is64 else 96)
    section_table = optional+optional_size
    sections = []
    for n in range(sections_count):
        start = section_table+n*40
        virtual_size,rva,raw_size,raw_offset = struct.unpack_from('<IIII',data,start+8)
        sections.append((rva,max(virtual_size,raw_size),raw_offset))
    def raw(rva):
        for start,size,offset in sections:
            if start <= rva < start+size:
                return offset+rva-start
        raise ValueError('RVA outside image sections')
    debug_rva, debug_size = struct.unpack_from('<II',data,directories+6*8)
    for n in range(debug_size//28):
        start = raw(debug_rva)+n*28
        kind,size,_,offset = struct.unpack_from('<IIII',data,start+12)
        if kind == 2 and data[offset:offset+4] == b'RSDS':
            guid = uuid.UUID(bytes_le=data[offset+4:offset+20])
            age = struct.unpack_from('<I',data,offset+20)[0]
            pdb = data[offset+24:offset+size].split(b'\0')[0].decode().replace('\\','/').split('/')[-1]
            return {'image':str(Path(path).resolve()),'pdb':pdb,'guid':guid.hex.upper(),'age':age,'sections':sections}
    raise ValueError('No RSDS signature')


class Pdb:
    def __init__(self, data):
        self.data = data
        assert data.startswith(b'Microsoft C/C++ MSF 7.00')
        self.block_size,_,_,directory_size,_,block_map = struct.unpack_from('<6I',data,32)
        block_count = (directory_size+self.block_size-1)//self.block_size
        blocks = struct.unpack_from('<'+str(block_count)+'I',data,block_map*self.block_size)
        directory = self.blocks(blocks)[:directory_size]
        stream_count = struct.unpack_from('<I',directory)[0]
        sizes = struct.unpack_from('<'+str(stream_count)+'I',directory,4)
        position = 4+4*stream_count
        self.streams = []
        for size in sizes:
            count = 0 if size == 0xffffffff else (size+self.block_size-1)//self.block_size
            blocks = struct.unpack_from('<'+str(count)+'I',directory,position)
            position += 4*count
            self.streams.append((size,blocks))

    def blocks(self, blocks):
        return b''.join(self.data[n*self.block_size:(n+1)*self.block_size] for n in blocks)

    def stream(self, n):
        size,blocks = self.streams[n]
        return self.blocks(blocks)[:size]


def inspect(image, cache, filters=None):
    metadata = pe_metadata(image)
    folder = cache/metadata['pdb']/(metadata['guid']+format(metadata['age'],'X'))
    folder.mkdir(parents=True,exist_ok=True)
    pdb_path = folder/metadata['pdb']
    if not pdb_path.exists():
        url = 'https://msdl.microsoft.com/download/symbols/'+metadata['pdb']+'/'+folder.name+'/'+metadata['pdb']
        print('Fetching matching public symbols:',url,flush=True)
        with urllib.request.urlopen(url,timeout=60) as response:
            pdb_path.write_bytes(response.read())
    pdb = Pdb(pdb_path.read_bytes())
    info = pdb.stream(1)
    assert uuid.UUID(bytes_le=info[12:28]).hex.upper() == metadata['guid']
    dbi = pdb.stream(3)
    # Public/stripped PDBs can have a newer info-stream age. The DBI age identifies
    # the linked image; retain both rather than silently accepting a GUID alone.
    metadata['pdb_info_age'] = struct.unpack_from('<I',info,8)[0]
    metadata['pdb_dbi_age'] = struct.unpack_from('<I',dbi,8)[0]
    assert metadata['pdb_dbi_age'] == metadata['age']
    records = pdb.stream(struct.unpack_from('<H',dbi,20)[0])
    undecorate = ctypes.WinDLL('dbghelp').UnDecorateSymbolName
    undecorate.argtypes = [ctypes.c_char_p,ctypes.c_char_p,ctypes.c_uint32,ctypes.c_uint32]
    undecorate.restype = ctypes.c_uint32
    symbols = []
    position = 0
    while position+4 <= len(records):
        size,kind = struct.unpack_from('<HH',records,position)
        if size < 2 or position+size+2 > len(records):
            raise ValueError('Invalid public symbol record')
        if kind == 0x110e:
            flags,offset,segment = struct.unpack_from('<IIH',records,position+4)
            name = records[position+14:position+size+2].split(b'\0')[0]
            if (filters and any(word.encode() in name for word in filters)) or (not filters and (any(word in name for word in [b'ItemPosition',b'OwnerDataCallback',b'AutoArrange',b'SetDesktopWorkAreas'])
                or b'CListViewHost' in name and any(word in name for word in [b'QueryInterface',b'GetWindow',b'GetHWND',b'GetListView']))):
                buffer = ctypes.create_string_buffer(4096)
                undecorate(name,buffer,len(buffer),0)
                if segment and segment <= len(metadata['sections']):
                    symbols.append({'rva':metadata['sections'][segment-1][0]+offset,'decorated':name.decode(errors='replace'),'name':buffer.value.decode(errors='replace')})
        position += size+2
    metadata['symbols'] = symbols
    output = cache/(Path(image).name+('.filtered' if filters else '')+'.symbols.json')
    output.write_text(json.dumps(metadata,indent=2),encoding='utf-8')
    print('Verified PDB:',metadata['guid'],'symbols:',len(symbols),flush=True)
    for symbol in symbols:
        print(hex(symbol['rva']),symbol['name'] or symbol['decorated'])


if __name__ == '__main__':
    parser=argparse.ArgumentParser()
    parser.add_argument('image')
    parser.add_argument('--cache',default='target/symbols')
    parser.add_argument('--filter', action='append')
    arguments=parser.parse_args()
    inspect(arguments.image,Path(arguments.cache),arguments.filter)
