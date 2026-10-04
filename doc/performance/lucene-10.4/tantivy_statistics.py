"""Read the text token header through the actual Tantivy composite directory.

Used only by the historical statistics matcher. This is a stored header, not an
independent proof of exact collection statistics in old deletion-merged indexes.
"""
import json
import struct
from pathlib import Path


def read_vint(data, offset):
    value = 0
    for shift in range(0, 70, 7):
        if offset >= len(data):
            raise ValueError('Truncated composite VInt')
        byte = data[offset]
        offset += 1
        if shift == 63 and byte & 127 > 1:
            raise ValueError('Composite VInt overflow')
        value |= (byte & 127) << shift
        if byte & 128:
            return value, offset
    raise ValueError('Unterminated composite VInt')


def token_header(path, field_id):
    with Path(path).open('rb') as source:
        source.seek(0, 2)
        size = source.tell()
        if size < 12:
            raise ValueError('Truncated Tantivy postings file')
        source.seek(size - 8)
        footer_len, magic = struct.unpack('<II', source.read(8))
        if magic != 1337 or footer_len > 50_000 or footer_len > size - 12:
            raise ValueError('Invalid Tantivy outer footer')
        body_end = size - footer_len - 8
        source.seek(body_end)
        json.loads(source.read(footer_len))
        source.seek(body_end - 4)
        composite_len = struct.unpack('<I', source.read(4))[0]
        composite_start = body_end - 4 - composite_len
        if composite_start < 0:
            raise ValueError('Invalid composite footer length')
        source.seek(composite_start)
        directory = source.read(composite_len)
        count, cursor = read_vint(directory, 0)
        entries = []
        offset = 0
        for _ in range(count):
            delta, cursor = read_vint(directory, cursor)
            offset += delta
            if cursor + 4 > len(directory):
                raise ValueError('Truncated composite field ID')
            field = struct.unpack_from('<I', directory, cursor)[0]
            cursor += 4
            idx, cursor = read_vint(directory, cursor)
            if offset > composite_start or (field, idx) in [key for key, _ in entries]:
                raise ValueError('Invalid composite offsets or duplicate address')
            entries.append(((field, idx), offset))
        if cursor != len(directory):
            raise ValueError('Trailing composite directory bytes')
        for position, (address, start) in enumerate(entries):
            if address == (field_id, 0):
                end = entries[position + 1][1] if position + 1 < len(entries) else composite_start
                if end - start < 8:
                    raise ValueError('Truncated text token header')
                source.seek(start)
                return struct.unpack('<Q', source.read(8))[0]
        raise ValueError('Text postings (field, idx=0) are absent')


def text_token_total(index):
    index = Path(index)
    metadata = json.loads((index / 'meta.json').read_text())
    fields = [i for i, field in enumerate(metadata['schema']) if field['name'] == 'text']
    if len(fields) != 1 or len(metadata['segments']) != 1:
        raise ValueError('Statistics matcher requires the frozen one-segment text index')
    segment = metadata['segments'][0]['segment_id'].replace('-', '')
    return token_header(index / f'{segment}.idx', fields[0])
