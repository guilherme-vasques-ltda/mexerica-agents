import test from 'node:test'
import assert from 'node:assert/strict'
import type { ForkCloudResult } from '../../shared/fork-cloud.ts'
import { managedAction } from './managed-connection.ts'
const result=(phase:string,more:Record<string,unknown>={}):ForkCloudResult=>({registration:'registered',recovery:null,state:{kind:'needsLink',epoch:1,attemptId:''},computers:[],bootstrap:{bootstrapId:'b',phase,blocksOtherOperations:true,canStart:false,canCheck:true,canProve:false,canReconcile:false,...more}} as ForkCloudResult)
test('managed login uses the actual device ceremony and connects only after complete, for the offered computer',()=>{
 assert.deepEqual(managedAction({...result('available'),registration:'unregistered'},'computer',true),{kind:'registerDevice',name:'Donnabot'})
 assert.deepEqual(managedAction(result('available'),'computer',true),{kind:'startBootstrap'})
 assert.deepEqual(managedAction(result('requested',{canProve:true}),'computer',true),{kind:'proveBootstrap',bootstrapId:'b'})
 assert.deepEqual(managedAction(result('receipt',{canReconcile:true}),'computer',true),{kind:'reconcileBootstrap',bootstrapId:'b'})
 assert.deepEqual(managedAction(result('complete',{computerId:'computer'}),'computer',true),{kind:'connect',computerId:'computer'})
 assert.equal(managedAction(result('complete',{computerId:'other'}),'computer',true),null)
})
test('restored uncertain writes read only, never repeat proof/create/publication; registration only reconciles',()=>{
 for(const phase of ['createUnknown','openingUnknown','challengeUnknown','challenged','proofUnknown','reconcileUnknown'])
  assert.deepEqual(managedAction(result(phase,{canReconcile:true}),'computer',true),{kind:'checkBootstrap',bootstrapId:'b'})
 assert.deepEqual(managedAction({...result('available'),recovery:'registration'},'computer',true),{kind:'reconcileRegistration'})
 assert.equal(managedAction({...result('available'),recovery:'enrollment'},'computer',true),null)
 assert.equal(managedAction(result('closed',{canCheck:false}),'computer',true),null)
})

test('janela inicial vencida só impede cadastro/vínculo novo; complete e recuperação conservam a mesma cadeia',()=>{
 assert.equal(managedAction({...result('available'),registration:'unregistered'},'computer',false),null)
 assert.equal(managedAction(result('available'),'computer',false),null)
 assert.equal(managedAction(result('prepared'),'computer',false),null)
 assert.deepEqual(managedAction(result('complete',{computerId:'computer'}),'computer',false),{kind:'connect',computerId:'computer'})
 assert.deepEqual(managedAction(result('proofUnknown',{canCheck:true,computerId:'computer'}),'computer',false),{kind:'checkBootstrap',bootstrapId:'b'})
 assert.equal(managedAction(result('complete',{computerId:'other'}),'computer',false),null)
})
