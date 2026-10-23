import pathlib,struct,shutil
root=pathlib.Path(__file__).resolve().parents[1]/'release/windows'
def imports(file):
 b=file.read_bytes();pe=struct.unpack_from('<I',b,60)[0];count=struct.unpack_from('<H',b,pe+6)[0];opts=struct.unpack_from('<H',b,pe+20)[0];base=pe+24;directory=base+(112 if struct.unpack_from('<H',b,base)[0]==0x20b else 96);sections=base+opts
 def offset(rva):
  for i in range(count):
   vsize,va,rawsize,raw=struct.unpack_from('<IIII',b,sections+i*40+8)
   if va<=rva<va+max(vsize,rawsize):return raw+rva-va
  return rva
 names=[]
 for index,step,nameoff in [(1,20,12),(13,32,4)]:
  rva,size=struct.unpack_from('<II',b,directory+index*8)
  if not rva:continue
  cursor=offset(rva)
  while any(b[cursor:cursor+step]):
   name=offset(struct.unpack_from('<I',b,cursor+nameoff)[0]);names.append(b[name:b.index(b'\0',name)].decode().lower());cursor+=step
 return names
files={p.name.lower():p for p in root.glob('*.dll')};pending=[root/'pulse-media.exe',*root.glob('plugins/*.dll')];seen=set();external=set()
while pending:
 file=pending.pop()
 if file in seen:continue
 seen.add(file)
 for name in imports(file):
  if name in files:pending.append(files[name])
  else:external.add(name)
unused=root.parent/'windows-unused';unused.mkdir(exist_ok=True)
for p in files.values():
 if p not in seen:shutil.move(str(p),str(unused/p.name))
print('Bundled libraries:',len(seen),'system dependencies:',','.join(sorted(external)))
