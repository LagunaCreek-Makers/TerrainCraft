//! Conservative orientation repair for already-closed triangle meshes.
//! Never invents missing geometry or modifies coordinates.
use std::collections::{HashMap,VecDeque};
use crate::mesh_audit::{audit,MeshAudit};
type Vertex=[u64;3];
type Edge=(Vertex,Vertex);
fn vertex(p:[f64;3])->Vertex {p.map(|v| if v==0.0 {0.0f64.to_bits()} else {v.to_bits()})}
fn edge(a:Vertex,b:Vertex)->(Edge,bool){if a<=b {((a,b),true)} else {((b,a),false)}}
#[derive(Debug,Clone)]
pub struct RepairReport {pub flipped_triangles:usize,pub before:MeshAudit,pub after:MeshAudit}
/// Reorient triangles using shared-edge adjacency. Refuses open, non-manifold,
/// degenerate, or non-orientable inputs. Does not fix self-intersections.
pub fn repair_winding(faces:&mut Vec<[[f64;3];3]>)->Result<RepairReport,String>{
    let before=audit(faces);
    if before.degenerate_triangles>0 || before.boundary_edges>0 || before.nonmanifold_edges>0 || faces.is_empty(){
        return Err(format!("Cannot repair winding: {} degenerate triangles, {} boundary edges, {} non-manifold edges",before.degenerate_triangles,before.boundary_edges,before.nonmanifold_edges));
    }
    let mut edges:HashMap<Edge,Vec<(usize,bool)>>=HashMap::new();
    for (i,t) in faces.iter().enumerate(){
        let v=t.map(vertex);
        for (a,b) in [(v[0],v[1]),(v[1],v[2]),(v[2],v[0])] {let (k,forward)=edge(a,b);edges.entry(k).or_default().push((i,forward));}
    }
    if edges.values().any(|v|v.len()!=2){return Err("Mesh contains edges not shared by exactly two triangles".into());}
    let mut adjacent=vec![Vec::<(usize,bool)>::new();faces.len()];
    for occurrences in edges.values(){
        let (a,dir_a)=occurrences[0];let (b,dir_b)=occurrences[1];
        if a==b{return Err("Triangle has repeated edge".into());}
        let must_differ=dir_a==dir_b;
        adjacent[a].push((b,must_differ));adjacent[b].push((a,must_differ));
    }
    let mut flips=vec![None;faces.len()];
    for start in 0..faces.len(){
        if flips[start].is_some(){continue;}
        flips[start]=Some(false);let mut queue=VecDeque::from([start]);let mut component=vec![start];
        while let Some(i)=queue.pop_front(){
            let current=flips[i].unwrap();
            for &(j,different) in &adjacent[i]{
                let desired=current^different;
                match flips[j]{Some(existing) if existing!=desired=>return Err("Non-orientable or contradictory mesh topology".into()),Some(_)=>(),None=>{flips[j]=Some(desired);queue.push_back(j);component.push(j);}}
            }
        }
        // The seed face's orientation is arbitrary; make each closed shell face outward
        // (positive signed volume). Assumes shells are not internal cavities.
        let volume:f64=component.iter().map(|&i|{
            let [a,b,c]=faces[i];
            let v=a[0]*(b[1]*c[2]-b[2]*c[1])-a[1]*(b[0]*c[2]-b[2]*c[0])+a[2]*(b[0]*c[1]-b[1]*c[0]);
            if flips[i]==Some(true){-v}else{v}
        }).sum();
        if volume<0.0{for &i in &component{flips[i]=flips[i].map(|f|!f);}}
    }
    let flipped_triangles=flips.iter().filter(|v|**v==Some(true)).count();
    for (face,flip) in faces.iter_mut().zip(flips){if flip==Some(true){face.swap(1,2);}}
    let after=audit(faces);
    if !after.is_closed_oriented_manifold(){return Err("Orientation repair did not yield a closed oriented manifold".into());}
    Ok(RepairReport{flipped_triangles,before,after})
}
#[cfg(test)] mod tests {
    use super::*;
    fn tetra()->Vec<[[f64;3];3]>{let a=[0.,0.,0.];let b=[1.,0.,0.];let c=[0.,1.,0.];let d=[0.,0.,1.];vec![[a,c,b],[a,b,d],[b,c,d],[c,a,d]]}
    #[test] fn fixes_reversed_triangle(){let mut f=tetra();f[0].swap(1,2);let r=repair_winding(&mut f).unwrap();assert_eq!(r.flipped_triangles,1);assert!(r.after.is_closed_oriented_manifold());}
    #[test] fn refuses_open_mesh(){let mut f=tetra();f.pop();assert!(repair_winding(&mut f).is_err());}
    #[test] fn preserves_good_mesh(){let mut f=tetra();assert_eq!(repair_winding(&mut f).unwrap().flipped_triangles,0);}
}
