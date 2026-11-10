"""Generate unit cells for the Archimedean tilings JETCARP lacks (edge length 1).

Each tiling is built from its large polygons placed on a lattice; equilateral triangles are then
found as unit triangles on the vertex pool that do not overlap a large polygon. One representative
of each polygon (centroid in the fundamental parallelogram) is kept. Run:
    python3 tools/gen_archimedean.py > crates/jetcarp-core/data/tilings/archimedean.json
"""
import json, math
S3 = math.sqrt(3)

def regular(c, n, r, a0):
    return [(c[0] + r*math.cos(a0 + 2*math.pi*k/n), c[1] + r*math.sin(a0 + 2*math.pi*k/n)) for k in range(n)]

def area(p):
    return abs(sum(p[i][0]*p[(i+1)%len(p)][1]-p[(i+1)%len(p)][0]*p[i][1] for i in range(len(p))))/2

def cen(p):
    return (sum(x for x,_ in p)/len(p), sum(y for _,y in p)/len(p))

def inside(pt, poly):
    c=False; n=len(poly); j=n-1
    for i in range(n):
        (xi,yi),(xj,yj)=poly[i],poly[j]
        if (yi>pt[1])!=(yj>pt[1]) and pt[0] < (xj-xi)*(pt[1]-yi)/(yj-yi)+xi: c=not c
        j=i
    return c

def lattice_copies(a1, a2, base, R=3):
    out=[]
    for i in range(-R,R+1):
        for j in range(-R,R+1):
            t=(i*a1[0]+j*a2[0], i*a1[1]+j*a2[1])
            for p in base: out.append([(x+t[0],y+t[1]) for x,y in p])
    return out

def add_triangles(big):
    pool={}
    for p in big:
        for v in p: pool[(round(v[0],6),round(v[1],6))]=v
    pts=list(pool.values())
    tris={}
    for i,a in enumerate(pts):
        for b in pts[i+1:]:
            if abs(math.dist(a,b)-1)>1e-6: continue
            m=((a[0]+b[0])/2,(a[1]+b[1])/2); d=(b[0]-a[0],b[1]-a[1])
            for s in (1,-1):
                c=(m[0]-s*d[1]*S3/2, m[1]+s*d[0]*S3/2)
                if (round(c[0],6),round(c[1],6)) not in pool: continue
                t=[a,b,pool[(round(c[0],6),round(c[1],6))]]
                g=cen(t)
                if any(inside(g,p) for p in big): continue
                tris[(round(g[0],5),round(g[1],5))]=t
    return big+list(tris.values())

def reduce_cell(a1,a2,polys):
    det=a1[0]*a2[1]-a2[0]*a1[1]
    out={}
    for p in polys:
        g=cen(p)
        u=(g[0]*a2[1]-a2[0]*g[1])/det; v=(a1[0]*g[1]-g[0]*a1[1])/det
        if -1e-7<=u<1-1e-7 and -1e-7<=v<1-1e-7:
            out[(round(g[0],5),round(g[1],5))]=[[x,y] for x,y in p]
    cell=list(out.values())
    tot=sum(area(p) for p in cell)
    assert abs(tot-abs(det))<1e-6, (tot,det)
    return cell

T=[]
def emit(id,name,note,a1,a2,base,tri=True):
    polys=lattice_copies(a1,a2,base)
    if tri: polys=add_triangles(polys)
    cell=reduce_cell(a1,a2,polys)
    T.append({"id":id,"name":name,"group":"Archimedean","k":1,"note":note,"a1":list(a1),"a2":list(a2),"polys":cell})

# 3.3.3.4.4 elongated triangular
emit("elongtri","3.3.3.4.4 elongated triangular","Rows of squares between rows of triangles.",
     (1,0),(0.5,1+S3/2),[[(0,0),(1,0),(1,1),(0,1)]])
# 3.4.6.4 rhombitrihexagonal
L=1+S3
hexa=regular((0,0),6,1,0)
sq=[]
for k in range(6):
    a,b=hexa[k],hexa[(k+1)%6]; n=(math.cos(math.pi/6+k*math.pi/3),math.sin(math.pi/6+k*math.pi/3))
    sq.append([a,b,(b[0]+n[0],b[1]+n[1]),(a[0]+n[0],a[1]+n[1])])
emit("rhombitrihex","3.4.6.4 rhombitrihexagonal","Hexagons ringed by squares, triangles in the gaps.",
     (L*math.cos(math.pi/6),L*math.sin(math.pi/6)),(0,L),[hexa]+sq)
# 3.12.12 truncated hexagonal
R12=1/(2*math.sin(math.pi/12)); A12=1/(2*math.tan(math.pi/12))
dod=regular((0,0),12,R12,math.pi/12)
emit("trunchex","3.12.12 truncated hexagonal","Dodecagons meeting edge to edge, triangles in the gaps.",
     (2*A12,0),(A12,2*A12*S3/2),[dod])
# 4.6.12 truncated trihexagonal
L=2*A12+1
sqs=[]
for k in range(0,12,2):
    a,b=dod[(k-1)%12],dod[k%12]
    ang=k*math.pi/6; n=(math.cos(ang),math.sin(ang))
    # edge between vertices at 15+30(k-1) and 15+30k degrees has normal at 30k
    sqs.append([a,b,(b[0]+n[0],b[1]+n[1]),(a[0]+n[0],a[1]+n[1])])
a1=(L,0); a2=(L/2,L*S3/2)
hexes=[]
for h in [((a1[0]+a2[0])/3,(a1[1]+a2[1])/3),(2*(a1[0]+a2[0])/3,2*(a1[1]+a2[1])/3)]:
    hexes.append(regular(h,6,1,0))
emit("trunctrihex","4.6.12 truncated trihexagonal","Dodecagons, hexagons, and squares; three sizes at every corner.",
     a1,a2,[dod]+sqs[:3]+hexes,tri=False)
# 3.3.3.3.6 snub hexagonal (chiral)
e1=(1,0); e2=(0.5,S3/2)
a1=(2*e1[0]+e2[0],2*e1[1]+e2[1]); a2=(-e1[0]+3*e2[0],-e1[1]+3*e2[1])
emit("snubhex","3.3.3.3.6 snub hexagonal","Hexagons in a sea of triangles. Chiral: it has a mirror twin.",
     a1,a2,[regular((0,0),6,1,0)])
# 3.3.4.3.4 snub square: solve the rotation so neighbouring squares share corners.
Ls=math.sqrt(2+S3)
def snub(th):
    A=regular((0,0),4,1/math.sqrt(2),th); B=regular((Ls/2,Ls/2),4,1/math.sqrt(2),-th)
    return A,B
best=None
for k in range(1,9000):
    th=k*math.pi/2/9000
    A,B=snub(th)
    d=min(math.dist(p,q) for p in A for q in B)
    if best is None or d<best[0]: best=(d,th)
lo,hi=best[1]-1e-3,best[1]+1e-3
for _ in range(200):
    m1,m2=lo+(hi-lo)/3,hi-(hi-lo)/3
    f=lambda th: min(math.dist(p,q) for p in snub(th)[0] for q in snub(th)[1])
    if f(m1)<f(m2): hi=m2
    else: lo=m1
A,B=snub((lo+hi)/2)
emit("snubsq","3.3.4.3.4 snub square","Squares turned alternately, joined by triangles.",(Ls,0),(0,Ls),[A,B])
for t in T:
    import sys; print(t["id"], len(t["polys"]), sorted(len(p) for p in t["polys"]), file=sys.stderr)
print(json.dumps({"source":"tools/gen_archimedean.py","tilings":T}))
